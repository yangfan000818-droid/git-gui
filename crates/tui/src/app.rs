//! ratatui 全屏 TUI:status 面板 + 冲突解决三栏视图 + stage 视图 + log 视图 + submodule 视图。
//!
//! 交互效果需在真实终端运行;逻辑尽量薄,核心都在 gitcore。

use crate::branch_ui::{self, BranchView};
use crate::cli::describe;
use crate::conflict_ui::{self, ConflictView};
use crate::log_ui::{self, LogView};
use crate::stage_ui::{self, StageView};
use crate::stash_ui::{self, StashView};
use crate::submodule_ui::{self, SubmoduleView};
use gitcore::{
    parse_repos_config, CancelToken, DiffOptions, IntegrationStrategy, Progress, Repo, RepoStatus,
    UpdateOptions, UpdateOutcome,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Gauge, Paragraph, Wrap};
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
    Branch(BranchView),
    Conflict(ConflictView),
    Stage(StageView),
    Stash(StashView),
    Log(LogView),
    Diff(String, u16), // diff 内容 + 纵向滚动偏移
    Submodule(SubmoduleView),
}

struct RepoEntry {
    name: String,
    #[allow(dead_code)]
    path: PathBuf,
    repo: Option<Repo>, // None = 打开失败
    status: Option<RepoStatus>,
}

// 后台操作(update / push)的流式消息:进度 + 最终结果。
enum OpMsg {
    Progress(Progress),
    Update(Result<UpdateOutcome, gitcore::Error>),
    Push(Result<gitcore::PushOutcome, gitcore::Error>),
}

// 后台执行中的操作:主循环轮询 rx 取进度/结果,Esc 置位 cancel 请求取消。
struct RunningOp {
    cancel: CancelToken,
    rx: std::sync::mpsc::Receiver<OpMsg>,
    handle: std::thread::JoinHandle<()>,
}

struct AppState {
    repos: Vec<RepoEntry>,
    current_index: usize,
    screen: Screen,
    message: String,
    /// 有后台操作(update / push)在跑时为 Some;其间只接受 Esc 取消。
    running: Option<RunningOp>,
    /// push 被拒触发的自动整合:整合干净完成后自动重推。
    push_after_integrate: bool,
    /// 当前整合策略(merge / rebase),按 m 切换。
    strategy: IntegrationStrategy,
    /// 后台操作的最新进度;有后台操作在跑时渲染为进度条 widget。
    progress: Option<Progress>,
    /// status 主屏状态文本的纵向滚动偏移。
    status_scroll: u16,
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
        running: None,
        push_after_integrate: false,
        strategy: IntegrationStrategy::Merge,
        progress: None,
        status_scroll: 0,
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
        poll_running(state);
        terminal.draw(|f| draw(f, state))?;
        // 后台 update 在跑时刷新更勤,及时反映完成 / 取消。
        let timeout = if state.running.is_some() {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        };
        if !event::poll(timeout)? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            // 后台更新进行中:只接受 Esc 取消,其余按键忽略以免改动状态。
            if state.running.is_some() {
                if key.code == KeyCode::Esc {
                    if let Some(r) = &state.running {
                        r.cancel.cancel();
                    }
                    state.message = "正在取消…".into();
                }
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
                KeyCode::Up => {
                    if dispatch(state, crate::keys::UP) {
                        return Ok(());
                    }
                }
                KeyCode::Down => {
                    if dispatch(state, crate::keys::DOWN) {
                        return Ok(());
                    }
                }
                KeyCode::Left => {
                    if dispatch(state, crate::keys::LEFT) {
                        return Ok(());
                    }
                }
                KeyCode::Right => {
                    if dispatch(state, crate::keys::RIGHT) {
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

// 后台操作:drain 进度更新 message;拿到最终结果(update / push)则落地。
fn poll_running(state: &mut AppState) {
    let final_msg = loop {
        let msg = match &state.running {
            None => return,
            Some(op) => op.rx.try_recv(),
        };
        match msg {
            Ok(OpMsg::Progress(p)) => state.progress = Some(p),
            Ok(done) => break done,
            Err(std::sync::mpsc::TryRecvError::Empty) => return, // 还在跑
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                state.running = None;
                state.push_after_integrate = false;
                state.message = "后台操作意外退出".into();
                return;
            }
        }
    };
    if let Some(op) = state.running.take() {
        let _ = op.handle.join();
    }
    state.progress = None;
    match final_msg {
        OpMsg::Update(Ok(outcome)) => after_integration(state, outcome),
        OpMsg::Update(Err(gitcore::Error::Cancelled)) => {
            state.push_after_integrate = false;
            state.message = "更新已取消".into();
            reload(state);
        }
        OpMsg::Update(Err(e)) => {
            state.push_after_integrate = false;
            state.message = format!("更新失败: {e}");
        }
        OpMsg::Push(result) => after_push(state, result),
        OpMsg::Progress(_) => {}
    }
}

fn draw(f: &mut Frame, state: &AppState) {
    match &state.screen {
        Screen::Status => draw_status(f, state),
        Screen::Branch(view) => view.render(f),
        Screen::Stash(view) => view.render(f),
        Screen::Conflict(view) => view.render(f),
        Screen::Stage(view) => view.render(f),
        Screen::Log(view) => view.render(f),
        Screen::Diff(content, scroll) => draw_diff(f, content, *scroll),
        Screen::Submodule(view) => view.render(f),
    }
}

// 处理一次按键;返回 true 表示退出程序。
fn dispatch(state: &mut AppState, c: char) -> bool {
    // 把 screen 取出来(默认回落 Status),处理后各分支显式设回。
    match std::mem::replace(&mut state.screen, Screen::Status) {
        Screen::Status => match c {
            'q' => return true,
            'j' | crate::keys::DOWN => {
                let max = status_body_lines(state).saturating_sub(1) as u16;
                state.status_scroll = (state.status_scroll + 1).min(max);
            }
            'k' | crate::keys::UP => state.status_scroll = state.status_scroll.saturating_sub(1),
            'r' => {
                reload(state);
                state.message = "已刷新".into();
            }
            'b' => {
                if let Some(repo) = state.current_repo() {
                    match BranchView::load(repo) {
                        Ok(v) => state.screen = Screen::Branch(v),
                        Err(e) => state.message = format!("加载分支失败: {e}"),
                    }
                }
            }
            'h' => {
                if let Some(repo) = state.current_repo() {
                    match StashView::load(repo) {
                        Ok(v) => state.screen = Screen::Stash(v),
                        Err(e) => state.message = format!("加载 Stash 失败: {e}"),
                    }
                }
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
                                state.screen = Screen::Diff(content, 0);
                            }
                        }
                        Err(e) => state.message = format!("查看 diff 失败: {e}"),
                    }
                }
            }
            'p' => spawn_push(state),
            'u' => spawn_update(state, false),
            'm' => {
                state.strategy = match state.strategy {
                    IntegrationStrategy::Merge => IntegrationStrategy::Rebase,
                    IntegrationStrategy::Rebase => IntegrationStrategy::Merge,
                };
                state.message = format!("整合策略已切换为 {}", strategy_label(state.strategy));
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
        Screen::Branch(mut view) => {
            if let Some(repo) = state.current_repo() {
                match view.handle_key(repo, c) {
                    Ok(branch_ui::Action::Back) => {
                        reload(state);
                    }
                    Ok(branch_ui::Action::BranchesChanged) => {
                        reload(state);
                        state.screen = Screen::Branch(view);
                    }
                    Ok(branch_ui::Action::None) => state.screen = Screen::Branch(view),
                    Err(e) => {
                        state.message = format!("操作失败: {e}");
                        state.screen = Screen::Branch(view);
                    }
                }
            }
        }
        Screen::Stash(mut view) => {
            if let Some(repo) = state.current_repo() {
                match view.handle_key(repo, c) {
                    Ok(stash_ui::Action::Back) => {
                        reload(state);
                    }
                    Ok(stash_ui::Action::StashChanged) => {
                        reload(state);
                        state.screen = Screen::Stash(view);
                    }
                    Ok(stash_ui::Action::None) => state.screen = Screen::Stash(view),
                    Err(e) => {
                        state.message = format!("操作失败: {e}");
                        state.screen = Screen::Stash(view);
                    }
                }
            }
        }
        Screen::Diff(content, scroll) => match c {
            'q' | '\x1b' => reload(state),
            'j' | crate::keys::DOWN => {
                let max = content.lines().count().saturating_sub(1) as u16;
                state.screen = Screen::Diff(content, (scroll + 1).min(max));
            }
            'k' | crate::keys::UP => state.screen = Screen::Diff(content, scroll.saturating_sub(1)),
            _ => state.screen = Screen::Diff(content, scroll),
        },
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
            let repo = state.current_repo().cloned();
            if let Some(repo) = repo {
                match view.handle_key(&repo, c) {
                    Ok(conflict_ui::Action::Quit) => return true,
                    Ok(conflict_ui::Action::Continue(autostash)) => {
                        match repo.continue_update(autostash) {
                            Ok(outcome) => after_integration(state, outcome),
                            Err(e) => {
                                state.push_after_integrate = false;
                                state.message = format!("完成失败: {e}");
                                reload(state);
                            }
                        }
                    }
                    Ok(conflict_ui::Action::Abort(autostash)) => {
                        state.push_after_integrate = false;
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

// 启动后台 update;push_after=true 表示由 push 被拒触发,整合干净完成后自动重推。
fn spawn_update(state: &mut AppState, push_after: bool) {
    let repo = match state.current_repo() {
        Some(r) => r.clone(),
        None => return,
    };
    let opts = UpdateOptions {
        strategy: state.strategy,
        ignore_whitespace: true,
    };
    let cancel = CancelToken::default();
    let token = cancel.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let mut on_progress = move |p: Progress| {
            let _ = progress_tx.send(OpMsg::Progress(p));
        };
        let result = repo.execute_update_streaming(&opts, &mut on_progress, &token);
        let _ = tx.send(OpMsg::Update(result));
    });
    state.running = Some(RunningOp { cancel, rx, handle });
    state.push_after_integrate = push_after;
    state.progress = None;
    state.message = if push_after {
        "远端领先,正在自动整合…(Esc 取消)".into()
    } else {
        "更新中…(Esc 取消)".into()
    };
}

// 启动后台 push(流式进度,可取消);结果在 after_push 落地。
fn spawn_push(state: &mut AppState) {
    let repo = match state.current_repo() {
        Some(r) => r.clone(),
        None => return,
    };
    let cancel = CancelToken::default();
    let token = cancel.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let mut on_progress = move |p: Progress| {
            let _ = progress_tx.send(OpMsg::Progress(p));
        };
        let result = repo.push_streaming(&mut on_progress, &token);
        let _ = tx.send(OpMsg::Push(result));
    });
    state.running = Some(RunningOp { cancel, rx, handle });
    state.progress = None;
    state.message = "推送中…(Esc 取消)".into();
}

// 落地一次 update / continue 的结果;若由 push 触发且整合干净完成,则自动重推。
fn after_integration(state: &mut AppState, outcome: UpdateOutcome) {
    match outcome {
        UpdateOutcome::Conflicted { files, autostash } => {
            // 冲突:进解决视图,push_after 标记留到解决后(continue_update 完成时再推)。
            enter_conflict(state, files, autostash);
        }
        UpdateOutcome::StashRestoreConflict { .. } => {
            // autostash 贴回有冲突 → 工作区不干净,不自动推。
            state.push_after_integrate = false;
            state.message = "整合成功,但 autostash 贴回有冲突,请手动处理".into();
            reload(state);
        }
        other => {
            state.message = describe(&other);
            reload(state);
            if state.push_after_integrate {
                // 保留标记:push 完成后 after_push 据此定 message,届时再清。
                spawn_push(state);
            }
        }
    }
}

// 落地 push 结果;远端领先则自动整合(push_after),整合干净后会再次 spawn_push。
fn after_push(state: &mut AppState, result: Result<gitcore::PushOutcome, gitcore::Error>) {
    match result {
        Ok(gitcore::PushOutcome::Success) => {
            state.message = if state.push_after_integrate {
                "整合后推送成功".into()
            } else {
                "推送成功".into()
            };
            state.push_after_integrate = false;
            reload(state);
        }
        Ok(gitcore::PushOutcome::NoUpstream) => {
            state.push_after_integrate = false;
            state.message = "无 upstream,请先执行 git push -u origin <branch>".into();
        }
        Ok(gitcore::PushOutcome::NonFastForward) => {
            // 远端领先:自动整合,完成后再推。
            spawn_update(state, true);
        }
        Err(gitcore::Error::Cancelled) => {
            state.push_after_integrate = false;
            state.message = "推送已取消".into();
            reload(state);
        }
        Err(e) => {
            state.push_after_integrate = false;
            state.message = format!("推送失败: {e}");
        }
    }
}

fn strategy_label(s: IntegrationStrategy) -> &'static str {
    match s {
        IntegrationStrategy::Merge => "merge",
        IntegrationStrategy::Rebase => "rebase",
    }
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
    state.status_scroll = 0;
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
        Paragraph::new(body)
            .block(Block::bordered().title(" 仓库状态 "))
            .scroll((state.status_scroll, 0)),
        chunks[1],
    );

    draw_footer(f, state, chunks[2]);
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
        Paragraph::new(body)
            .block(Block::bordered().title(" 仓库状态 "))
            .scroll((state.status_scroll, 0)),
        chunks[1],
    );

    draw_footer(f, state, chunks[2]);
}

// 底部栏:有后台操作(push / update)在跑时渲染进度条 widget,否则渲染消息 + 快捷键提示。
fn draw_footer(f: &mut Frame, state: &AppState, area: ratatui::layout::Rect) {
    use ratatui::style::{Color, Style};
    if state.running.is_some() {
        let (ratio, label) = match &state.progress {
            Some(p) => {
                let label = match p.percent {
                    Some(pct) => format!("{} {pct}%", p.phase),
                    None => format!("{}…", p.phase),
                };
                (p.percent.unwrap_or(0) as f64 / 100.0, label)
            }
            None => (0.0, "准备中…".to_string()),
        };
        f.render_widget(
            Gauge::default()
                .block(Block::bordered().title(" 进行中 · Esc 取消 "))
                .gauge_style(Style::default().fg(Color::Cyan))
                .ratio(ratio.clamp(0.0, 1.0))
                .label(label),
            area,
        );
        return;
    }
    f.render_widget(
        Paragraph::new(state.message.clone()).block(Block::bordered().title(format!(
            " s Stage · b Branch · h Stash · S 子仓库 · l Log · d Diff · p Push · u 更新 · m 策略[{}] · r 刷新 · q 退出 ",
            strategy_label(state.strategy)
        ))),
        area,
    );
}

// 当前仓库状态文本的行数,用于 status 滚动钳制。
fn status_body_lines(state: &AppState) -> usize {
    state
        .repos
        .get(state.current_index)
        .and_then(|e| e.status.as_ref())
        .map(|st| status_text(st).lines().count())
        .unwrap_or(0)
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

fn draw_diff(f: &mut Frame, content: &str, scroll: u16) {
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
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new("j/k 滚动 · q/Esc 返回").block(Block::bordered()),
        chunks[2],
    );
}
