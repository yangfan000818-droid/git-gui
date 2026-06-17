//! ratatui 全屏 TUI:status 面板 + 冲突解决三栏视图 + stage 视图 + log 视图 + submodule 视图。
//!
//! 交互效果需在真实终端运行;逻辑尽量薄,核心都在 gitcore。

use crate::cli::describe;
use crate::conflict_ui::{self, ConflictView};
use crate::log_ui::{self, LogView};
use crate::stage_ui::{self, StageView};
use crate::submodule_ui::{self, SubmoduleView};
use gitcore::{parse_repos_config, DiffOptions, Repo, RepoStatus, UpdateOptions, UpdateOutcome};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

pub fn run() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());

    // 加载仓库列表
    let repos = load_repos(&cwd);
    if repos.is_empty() {
        eprintln!("没有可用的仓库");
        return ExitCode::FAILURE;
    }

    match run_app(repos) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("TUI 错误: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 加载仓库列表(配置文件 + submodules)。
fn load_repos(cwd: &PathBuf) -> Vec<(String, PathBuf)> {
    let mut repos = Vec::new();

    // 1. 尝试加载配置文件
    let config_path = cwd.join(".git-gui/repos.toml");
    if config_path.exists() {
        if let Ok(configs) = parse_repos_config(&config_path) {
            for cfg in configs {
                let abs_path = if cfg.path.is_absolute() {
                    cfg.path
                } else {
                    cwd.join(&cfg.path)
                };
                repos.push((cfg.name, abs_path));
            }
        }
    }

    // 2. 如果配置文件没有主仓库,添加当前目录
    if repos.is_empty() || !repos.iter().any(|(_, p)| p == cwd) {
        repos.insert(0, ("主仓库".into(), cwd.clone()));
    }

    // 3. 合并 submodules
    if let Ok(repo) = Repo::open(cwd) {
        if let Ok(subs) = repo.submodules() {
            for sub in subs {
                let sub_path = cwd.join(&sub.path);
                // 去重：配置文件优先
                if !repos.iter().any(|(_, p)| p == &sub_path) {
                    repos.push((sub.name, sub_path));
                }
            }
        }
    }

    repos
}

enum Screen {
    Status,
    Conflict(ConflictView),
    Stage(StageView),
    Log(LogView),
    Diff(String), // diff 内容
    Submodule(SubmoduleView),
}

struct RepoEntry {
    name: String,
    #[allow(dead_code)]
    path: PathBuf,
    repo: Option<Repo>, // None = 打开失败
    status: Option<RepoStatus>,
}

struct AppState {
    repos: Vec<RepoEntry>,
    current_index: usize,
    screen: Screen,
    message: String,
}

impl AppState {
    fn current_repo(&self) -> Option<&Repo> {
        self.repos.get(self.current_index)?.repo.as_ref()
    }
}

fn run_app(repo_list: Vec<(String, PathBuf)>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut repos = Vec::new();
    for (name, path) in repo_list {
        let repo = Repo::open(&path).ok();
        repos.push(RepoEntry {
            name,
            path,
            repo,
            status: None,
        });
    }

    let mut state = AppState {
        repos,
        current_index: 0,
        screen: Screen::Status,
        message: "就绪".into(),
    };
    reload(&mut state);

    let res = event_loop(&mut terminal, &mut state);

    // 无论成败都还原终端,避免把用户终端搞坏。
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, state: &mut AppState) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, state))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char(c) => {
                    if dispatch(state, c) {
                        return Ok(());
                    }
                }
                KeyCode::Enter => {
                    if dispatch(state, '\n') {
                        return Ok(());
                    }
                }
                KeyCode::Esc => {
                    if dispatch(state, '\x1b') {
                        return Ok(());
                    }
                }
                KeyCode::Backspace => {
                    if dispatch(state, '\x08') {
                        return Ok(());
                    }
                }
                KeyCode::Delete => {
                    if dispatch(state, '\x7f') {
                        return Ok(());
                    }
                }
                // Tab 切换仓库
                KeyCode::Tab => {
                    state.current_index = (state.current_index + 1) % state.repos.len();
                    reload(state);
                    state.message = format!("切换到: {}", state.repos[state.current_index].name);
                }
                _ => {}
            }
        }
    }
}

fn draw(f: &mut Frame, state: &AppState) {
    match &state.screen {
        Screen::Status => draw_status(f, state),
        Screen::Conflict(view) => view.render(f),
        Screen::Stage(view) => view.render(f),
        Screen::Log(view) => view.render(f),
        Screen::Diff(content) => draw_diff(f, content),
        Screen::Submodule(view) => view.render(f),
    }
}

// 处理一次按键;返回 true 表示退出程序。
fn dispatch(state: &mut AppState, c: char) -> bool {
    // 把 screen 取出来(默认回落 Status),处理后各分支显式设回。
    match std::mem::replace(&mut state.screen, Screen::Status) {
        Screen::Status => match c {
            'q' => return true,
            'r' => {
                reload(state);
                state.message = "已刷新".into();
            }
            's' => {
                if let Some(repo) = state.current_repo() {
                    match StageView::load(repo) {
                        Ok(v) => state.screen = Screen::Stage(v),
                        Err(e) => state.message = format!("进入 Stage 失败: {e}"),
                    }
                }
            }
            'S' => {
                // 大写 S：进入 Submodule 视图
                let current_status = state
                    .repos
                    .get(state.current_index)
                    .and_then(|e| e.status.as_ref());
                if let Some(st) = current_status {
                    if st.submodules.is_empty() {
                        state.message = "无子仓库".into();
                    } else {
                        state.screen =
                            Screen::Submodule(SubmoduleView::load(st.submodules.clone()));
                    }
                }
            }
            'l' => {
                if let Some(repo) = state.current_repo() {
                    match LogView::load(repo) {
                        Ok(v) => state.screen = Screen::Log(v),
                        Err(e) => state.message = format!("加载 Log 失败: {e}"),
                    }
                }
            }
            'd' => {
                if let Some(repo) = state.current_repo() {
                    match repo.diff(&DiffOptions::default()) {
                        Ok(content) => {
                            if content.trim().is_empty() {
                                state.message = "工作区无改动".into();
                            } else {
                                state.screen = Screen::Diff(content);
                            }
                        }
                        Err(e) => state.message = format!("查看 diff 失败: {e}"),
                    }
                }
            }
            'p' => {
                if let Some(repo) = state.current_repo() {
                    match repo.push() {
                        Ok(gitcore::PushOutcome::Success) => {
                            state.message = "推送成功".into();
                            reload(state);
                        }
                        Ok(gitcore::PushOutcome::NoUpstream) => {
                            state.message =
                                "无 upstream,请先执行 git push -u origin <branch>".into();
                        }
                        Ok(gitcore::PushOutcome::NonFastForward) => {
                            state.message = "非快进推送被拒,请先 pull".into();
                        }
                        Err(e) => state.message = format!("推送失败: {e}"),
                    }
                }
            }
            'u' => {
                if let Some(repo) = state.current_repo() {
                    match repo.execute_update(&UpdateOptions::default()) {
                        Ok(UpdateOutcome::Conflicted { files, autostash }) => {
                            enter_conflict(state, files, autostash)
                        }
                        Ok(outcome) => {
                            state.message = describe(&outcome);
                            reload(state);
                        }
                        Err(e) => state.message = format!("更新失败: {e}"),
                    }
                }
            }
            'R' => {
                if let Some(repo) = state.current_repo() {
                    match repo.resume_conflicts() {
                        Ok(Some((files, autostash))) => enter_conflict(state, files, autostash),
                        Ok(None) => state.message = "没有未完成的整合".into(),
                        Err(e) => state.message = format!("恢复失败: {e}"),
                    }
                }
            }
            _ => {}
        },
        Screen::Stage(mut view) => {
            if let Some(repo) = state.current_repo() {
                match view.handle_key(repo, c) {
                    Ok(stage_ui::Action::Back) => {
                        reload(state);
                    }
                    Ok(stage_ui::Action::Commit(sha)) => {
                        state.message = format!("已提交 {sha}");
                        reload(state);
                    }
                    Ok(stage_ui::Action::None) => state.screen = Screen::Stage(view),
                    Err(e) => {
                        state.message = format!("操作失败: {e}");
                        state.screen = Screen::Stage(view);
                    }
                }
            }
        }
        Screen::Log(mut view) => {
            if let Some(repo) = state.current_repo() {
                match view.handle_key(repo, c) {
                    Ok(log_ui::Action::Back) => {
                        reload(state);
                    }
                    Ok(log_ui::Action::None) => state.screen = Screen::Log(view),
                    Err(e) => {
                        state.message = format!("操作失败: {e}");
                        state.screen = Screen::Log(view);
                    }
                }
            }
        }
        Screen::Diff(_) => {
            if c == 'q' {
                reload(state);
            } else {
                state.screen = std::mem::replace(&mut state.screen, Screen::Status);
            }
        }
        Screen::Submodule(mut view) => match view.handle_key(c) {
            submodule_ui::Action::Back => {
                reload(state);
            }
            submodule_ui::Action::SwitchTo(path) => {
                // 切换到子仓库：退出当前 TUI，重新启动到子仓库路径
                // TODO: 实现真正的切换（需要重构 run_app 接受路径参数）
                state.message = format!("切换子仓库功能开发中: {}", path.display());
                reload(state);
            }
            submodule_ui::Action::None => state.screen = Screen::Submodule(view),
        },
        Screen::Conflict(mut view) => {
            if let Some(repo) = state.current_repo() {
                match view.handle_key(repo, c) {
                    Ok(conflict_ui::Action::Quit) => return true,
                    Ok(conflict_ui::Action::Continue(autostash)) => {
                        match repo.continue_update(autostash) {
                            Ok(UpdateOutcome::Conflicted { files, autostash }) => {
                                enter_conflict(state, files, autostash);
                                state.message = "还有未解决的冲突".into();
                            }
                            Ok(outcome) => {
                                state.message = describe(&outcome);
                                reload(state);
                            }
                            Err(e) => {
                                state.message = format!("完成失败: {e}");
                                reload(state);
                            }
                        }
                    }
                    Ok(conflict_ui::Action::Abort(autostash)) => {
                        state.message = match repo.abort_update(autostash) {
                            Ok(()) => "已放弃整合".into(),
                            Err(e) => format!("放弃失败: {e}"),
                        };
                        reload(state);
                    }
                    Ok(conflict_ui::Action::None) => state.screen = Screen::Conflict(view),
                    Err(e) => {
                        state.message = format!("操作失败: {e}");
                        state.screen = Screen::Conflict(view);
                    }
                }
            }
        }
    }
    false
}

fn enter_conflict(
    state: &mut AppState,
    files: Vec<std::path::PathBuf>,
    autostash: Option<gitcore::StashRef>,
) {
    if let Some(repo) = state.current_repo() {
        match ConflictView::load(repo, files, autostash) {
            Ok(v) => state.screen = Screen::Conflict(v),
            Err(e) => state.message = format!("加载冲突失败: {e}"),
        }
    }
}

fn reload(state: &mut AppState) {
    state.screen = Screen::Status;
    let idx = state.current_index;
    if let Some(entry) = state.repos.get_mut(idx) {
        if let Some(ref repo) = entry.repo {
            match repo.status() {
                Ok(st) => entry.status = Some(st),
                Err(e) => state.message = format!("读取状态失败: {e}"),
            }
        }
    }
}

fn draw_status(f: &mut Frame, state: &AppState) {
    // 如果只有一个仓库，不显示左侧边栏
    if state.repos.len() == 1 {
        draw_status_single(f, state);
        return;
    }

    // 多仓库：左侧边栏 + 右侧状态
    let main_chunks = Layout::horizontal([
        Constraint::Length(30), // 左侧仓库列表
        Constraint::Min(40),    // 右侧状态
    ])
    .split(f.area());

    // 左侧仓库列表
    draw_repo_list(f, state, main_chunks[0]);

    // 右侧状态面板
    draw_status_panel(f, state, main_chunks[1]);
}

fn draw_status_single(f: &mut Frame, state: &AppState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(3),
    ])
    .split(f.area());

    f.render_widget(Paragraph::new(" git-gui · git 助手"), chunks[0]);

    let current_status = state
        .repos
        .get(state.current_index)
        .and_then(|e| e.status.as_ref());
    let body = match current_status {
        Some(st) => status_text(st),
        None => "加载中…".into(),
    };
    f.render_widget(
        Paragraph::new(body).block(Block::bordered().title(" 仓库状态 ")),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(state.message.clone())
            .block(Block::bordered().title(
                " s Stage · S 子仓库 · l Log · d Diff · p Push · u 更新 · r 刷新 · q 退出 ",
            )),
        chunks[2],
    );
}

fn draw_repo_list(f: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};

    let mut lines = Vec::new();
    for (i, entry) in state.repos.iter().enumerate() {
        let (status_icon, status_color) = if entry.repo.is_none() {
            ("✗", Color::Red)
        } else if let Some(ref st) = entry.status {
            if !st.conflicted.is_empty() {
                ("!", Color::Red)
            } else if st.dirty {
                ("●", Color::Yellow)
            } else {
                ("✓", Color::Green)
            }
        } else {
            ("?", Color::Gray)
        };

        let mut style = Style::default();
        if i == state.current_index {
            style = style.add_modifier(Modifier::REVERSED);
        }

        let line = Line::from(vec![
            Span::styled(status_icon, Style::default().fg(status_color)),
            Span::styled(format!(" {}", entry.name), style),
        ]);
        lines.push(line);
    }

    f.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" 仓库 (Tab切换) ")),
        area,
    );
}

fn draw_status_panel(f: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(3),
    ])
    .split(area);

    let current_repo_name = state
        .repos
        .get(state.current_index)
        .map(|e| e.name.as_str())
        .unwrap_or("?");
    f.render_widget(
        Paragraph::new(format!(" {} ", current_repo_name)),
        chunks[0],
    );

    let current_status = state
        .repos
        .get(state.current_index)
        .and_then(|e| e.status.as_ref());
    let body = match current_status {
        Some(st) => status_text(st),
        None => "加载中…".into(),
    };
    f.render_widget(
        Paragraph::new(body).block(Block::bordered().title(" 仓库状态 ")),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(state.message.clone())
            .block(Block::bordered().title(
                " s Stage · S 子仓库 · l Log · d Diff · p Push · u 更新 · r 刷新 · q 退出 ",
            )),
        chunks[2],
    );
}

fn status_text(st: &RepoStatus) -> String {
    let mut s = format!(
        "分支:      {}\nupstream:  {}\n领先/落后: +{} / -{}\n工作区:    {}",
        st.branch.as_deref().unwrap_or("(detached)"),
        st.upstream.as_deref().unwrap_or("(none)"),
        st.ahead,
        st.behind,
        if st.dirty {
            "有未提交改动"
        } else {
            "干净"
        },
    );
    if !st.conflicted.is_empty() {
        s.push_str(&format!(
            "\n冲突文件:  {} 个(按 R 恢复解决)",
            st.conflicted.len()
        ));
    }
    if !st.submodules.is_empty() {
        s.push_str(&format!("\n\n子仓库:    {} 个", st.submodules.len()));
        for sub in &st.submodules {
            let status_icon = match sub.status {
                gitcore::SubmoduleStatus::Clean => "✓",
                gitcore::SubmoduleStatus::Dirty => "●",
                gitcore::SubmoduleStatus::Detached => "⚠",
                gitcore::SubmoduleStatus::Uninitialized => "?",
            };
            s.push_str(&format!("\n  {} {}", status_icon, sub.name));
        }
    }
    s
}

fn draw_diff(f: &mut Frame, content: &str) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .split(f.area());

    f.render_widget(Paragraph::new(" Diff 视图 · 工作区改动"), chunks[0]);

    f.render_widget(
        Paragraph::new(content)
            .block(Block::bordered().title(" Diff "))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );

    f.render_widget(Paragraph::new("q 返回").block(Block::bordered()), chunks[2]);
}
