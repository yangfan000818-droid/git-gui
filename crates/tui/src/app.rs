//! ratatui 全屏 TUI:status 面板 + 按键触发 update。
//!
//! 交互效果需在真实终端运行;这里逻辑尽量薄,核心都在 gitcore。

use crate::cli::describe;
use gitcore::{Repo, RepoStatus, UpdateOptions};
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

struct AppState {
    status: Option<RepoStatus>,
    message: String,
}

fn run_app(repo: &Repo) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut state = AppState {
        status: None,
        message: "就绪".into(),
    };
    reload(repo, &mut state);

    let res = event_loop(&mut terminal, repo, &mut state);

    // 无论 event_loop 成败,都要还原终端,避免把用户终端搞坏。
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
        terminal.draw(|f| ui(f, state))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('r') => {
                    reload(repo, state);
                    state.message = "已刷新".into();
                }
                KeyCode::Char('u') => {
                    match repo.execute_update(&UpdateOptions::default()) {
                        Ok(outcome) => state.message = describe(&outcome),
                        Err(e) => state.message = format!("更新失败: {e}"),
                    }
                    reload(repo, state); // 刷新数字,保留 message
                }
                _ => {}
            }
        }
    }
}

fn reload(repo: &Repo, state: &mut AppState) {
    match repo.status() {
        Ok(st) => state.status = Some(st),
        Err(e) => state.message = format!("读取状态失败: {e}"),
    }
}

fn ui(f: &mut Frame, state: &AppState) {
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
            .block(Block::bordered().title(" u 更新 · r 刷新 · q 退出 ")),
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
        s.push_str(&format!("\n冲突文件:  {} 个", st.conflicted.len()));
    }
    s
}
