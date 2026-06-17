//! Stage 视图:列出文件状态,j/k 移动,Space 切换暂存,a 全暂存,c 提交。

use gitcore::{Error, FileState, FileStatus, Repo};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Stage 视图的动作。
pub enum Action {
    None,
    Commit(String), // 提交,附带 SHA
    Back,           // 返回 Status
}

enum Mode {
    FileList,
    CommitInput,
}

pub struct StageView {
    files: Vec<FileStatus>,
    cursor: usize,
    mode: Mode,
    commit_input: String,
    message: String,
}

impl StageView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let st = repo.status()?;
        Ok(StageView {
            files: st.files,
            cursor: 0,
            mode: Mode::FileList,
            commit_input: String::new(),
            message: "Space 暂存/取消 · a 全暂存 · c 提交 · q 返回".into(),
        })
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match self.mode {
            Mode::FileList => self.handle_file_list(repo, c),
            Mode::CommitInput => self.handle_commit_input(repo, c),
        }
    }

    fn handle_file_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' => return Ok(Action::Back),
            'j' if self.cursor + 1 < self.files.len() => self.cursor += 1,
            'k' if self.cursor > 0 => self.cursor -= 1,
            ' ' => self.toggle_stage(repo)?,
            'a' => {
                repo.stage_all()?;
                self.reload(repo)?;
                self.message = "已暂存全部".into();
            }
            'c' => {
                self.mode = Mode::CommitInput;
                self.commit_input.clear();
                self.message = "输入 commit 消息(Enter 提交 / Esc 取消)".into();
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_commit_input(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            '\x1b' => {
                // Esc
                self.mode = Mode::FileList;
                self.message = "已取消提交".into();
            }
            '\n' | '\r' => {
                // Enter
                if self.commit_input.trim().is_empty() {
                    self.message = "提交消息不能为空".into();
                    return Ok(Action::None);
                }
                let opts = gitcore::CommitOptions {
                    message: self.commit_input.clone(),
                    allow_empty: false,
                };
                match repo.commit(&opts) {
                    Ok(sha) => return Ok(Action::Commit(sha)),
                    Err(e) => {
                        self.message = format!("提交失败: {e}");
                        self.mode = Mode::FileList;
                    }
                }
            }
            '\x7f' | '\x08' => {
                // Backspace
                self.commit_input.pop();
            }
            c if !c.is_control() => {
                self.commit_input.push(c);
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn toggle_stage(&mut self, repo: &Repo) -> Result<(), Error> {
        if self.files.is_empty() {
            return Ok(());
        }
        let file = &self.files[self.cursor];
        match file.state {
            FileState::Staged | FileState::StagedAndModified => {
                repo.unstage(&[&file.path])?;
                self.message = format!("已取消暂存 {}", file.path.display());
            }
            FileState::Modified | FileState::Untracked => {
                repo.stage(&[&file.path])?;
                self.message = format!("已暂存 {}", file.path.display());
            }
        }
        self.reload(repo)?;
        Ok(())
    }

    fn reload(&mut self, repo: &Repo) -> Result<(), Error> {
        let st = repo.status()?;
        self.files = st.files;
        if self.cursor >= self.files.len() && !self.files.is_empty() {
            self.cursor = self.files.len() - 1;
        }
        Ok(())
    }

    pub fn render(&self, f: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        f.render_widget(Paragraph::new(" Stage 视图 · 暂存/取消暂存文件"), chunks[0]);

        match self.mode {
            Mode::FileList => self.render_file_list(f, chunks[1]),
            Mode::CommitInput => self.render_commit_input(f, chunks[1]),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_file_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.files.is_empty() {
            lines.push(Line::from("(工作区干净,无文件改动)"));
        } else {
            for (i, file) in self.files.iter().enumerate() {
                let (prefix, color) = match file.state {
                    FileState::Staged => ("● ", Color::Green),
                    FileState::Modified => ("  ", Color::Red),
                    FileState::Untracked => ("? ", Color::Gray),
                    FileState::StagedAndModified => ("◐ ", Color::Yellow),
                };
                let name = file.path.display().to_string();
                let mut style = Style::default().fg(color);
                if i == self.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(name, style),
                ]));
            }
        }
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" 文件列表 ")),
            area,
        );
    }

    fn render_commit_input(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let text = format!("消息: {}", self.commit_input);
        f.render_widget(
            Paragraph::new(text).block(Block::bordered().title(" Commit 消息 ")),
            area,
        );
    }
}
