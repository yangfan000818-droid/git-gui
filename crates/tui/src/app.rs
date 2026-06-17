//! ratatui 全屏 TUI:status 面板 + 冲突解决三栏视图 + stage 视图。
//!
//! 交互效果需在真实终端运行;逻辑尽量薄,核心都在 gitcore。

use crate::cli::describe;
use crate::conflict_ui::{self, ConflictView};
use crate::stage_ui::{self, StageView};
use gitcore::{Repo, RepoStatus, UpdateOptions, UpdateOutcome};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, Terminal};
use std::io;
use std::process::ExitCode;
use std::time::Duration;

pub fn run() -> ExitCode {
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let repo = match Repo::open(&cwd) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("打开仓库失败: {e}");
            return ExitCode::FAILURE;
        }
    };
    match run_app(&repo) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("TUI 错误: {e}");
            ExitCode::FAILURE
        }
    }
}

enum Screen {
    Status,
    Conflict(ConflictView),
    Stage(StageView),
}

struct AppState {
    screen: Screen,
    status: Option<RepoStatus>,
    message: String,
}

fn run_app(repo: &Repo) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut state = AppState {
        screen: Screen::Status,
        status: None,
        message: "就绪".into(),
    };
    reload(repo, &mut state);

    let res = event_loop(&mut terminal, repo, &mut state);

    // 无论成败都还原终端,避免把用户终端搞坏。
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}

fn event_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    repo: &Repo,
    state: &mut AppState,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, state))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if let KeyCode::Char(c) = key.code {
                if dispatch(repo, state, c) {
                    return Ok(());
                }
            }
        }
    }
}

fn draw(f: &mut Frame, state: &AppState) {
    match &state.screen {
        Screen::Status => draw_status(f, state),
        Screen::Conflict(view) => view.render(f),
        Screen::Stage(view) => view.render(f),
    }
}

// 处理一次按键;返回 true 表示退出程序。
fn dispatch(repo: &Repo, state: &mut AppState, c: char) -> bool {
    // 把 screen 取出来(默认回落 Status),处理后各分支显式设回。
    match std::mem::replace(&mut state.screen, Screen::Status) {
        Screen::Status => match c {
            'q' => return true,
            'r' => {
                reload(repo, state);
                state.message = "已刷新".into();
            }
            's' => match StageView::load(repo) {
                Ok(v) => state.screen = Screen::Stage(v),
                Err(e) => state.message = format!("进入 Stage 失败: {e}"),
            },
            'p' => match repo.push() {
                Ok(gitcore::PushOutcome::Success) => {
                    state.message = "推送成功".into();
                    reload(repo, state);
                }
                Ok(gitcore::PushOutcome::NoUpstream) => {
                    state.message = "无 upstream,请先执行 git push -u origin <branch>".into();
                }
                Ok(gitcore::PushOutcome::NonFastForward) => {
                    state.message = "非快进推送被拒,请先 pull".into();
                }
                Err(e) => state.message = format!("推送失败: {e}"),
            },
            'u' => match repo.execute_update(&UpdateOptions::default()) {
                Ok(UpdateOutcome::Conflicted { files, autostash }) => {
                    enter_conflict(repo, state, files, autostash)
                }
                Ok(outcome) => {
                    state.message = describe(&outcome);
                    reload(repo, state);
                }
                Err(e) => state.message = format!("更新失败: {e}"),
            },
            'R' => match repo.resume_conflicts() {
                Ok(Some((files, autostash))) => enter_conflict(repo, state, files, autostash),
                Ok(None) => state.message = "没有未完成的整合".into(),
                Err(e) => state.message = format!("恢复失败: {e}"),
            },
            _ => {}
        },
        Screen::Stage(mut view) => match view.handle_key(repo, c) {
            Ok(stage_ui::Action::Back) => {
                reload(repo, state);
            }
            Ok(stage_ui::Action::Commit(sha)) => {
                state.message = format!("已提交 {sha}");
                reload(repo, state);
            }
            Ok(stage_ui::Action::None) => state.screen = Screen::Stage(view),
            Err(e) => {
                state.message = format!("操作失败: {e}");
                state.screen = Screen::Stage(view);
            }
        },
        Screen::Conflict(mut view) => match view.handle_key(repo, c) {
            Ok(conflict_ui::Action::Quit) => return true,
            Ok(conflict_ui::Action::Continue(autostash)) => match repo.continue_update(autostash) {
                Ok(UpdateOutcome::Conflicted { files, autostash }) => {
                    enter_conflict(repo, state, files, autostash);
                    state.message = "还有未解决的冲突".into();
                }
                Ok(outcome) => {
                    state.message = describe(&outcome);
                    reload(repo, state);
                }
                Err(e) => {
                    state.message = format!("完成失败: {e}");
                    reload(repo, state);
                }
            },
            Ok(conflict_ui::Action::Abort(autostash)) => {
                state.message = match repo.abort_update(autostash) {
                    Ok(()) => "已放弃整合".into(),
                    Err(e) => format!("放弃失败: {e}"),
                };
                reload(repo, state);
            }
            Ok(conflict_ui::Action::None) => state.screen = Screen::Conflict(view),
            Err(e) => {
                state.message = format!("操作失败: {e}");
                state.screen = Screen::Conflict(view);
            }
        },
    }
    false
}

fn enter_conflict(
    repo: &Repo,
    state: &mut AppState,
    files: Vec<std::path::PathBuf>,
    autostash: Option<gitcore::StashRef>,
) {
    match ConflictView::load(repo, files, autostash) {
        Ok(v) => state.screen = Screen::Conflict(v),
        Err(e) => state.message = format!("加载冲突失败: {e}"),
    }
}

fn reload(repo: &Repo, state: &mut AppState) {
    match repo.status() {
        Ok(st) => state.status = Some(st),
        Err(e) => state.message = format!("读取状态失败: {e}"),
    }
}

fn draw_status(f: &mut Frame, state: &AppState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(6),
        Constraint::Length(3),
    ])
    .split(f.area());

    f.render_widget(Paragraph::new(" git-gui · git 助手"), chunks[0]);

    let body = match &state.status {
        Some(st) => status_text(st),
        None => "加载中…".into(),
    };
    f.render_widget(
        Paragraph::new(body).block(Block::bordered().title(" 仓库状态 ")),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new(state.message.clone())
            .block(Block::bordered().title(" s Stage · p Push · u 更新 · r 刷新 · q 退出 ")),
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
    s
}
