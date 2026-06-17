//! Stash 视图:列出、创建、应用、弹出、丢弃 stash。

use gitcore::{Error, Repo, StashEntry};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Stash 视图的动作。
pub enum Action {
    None,
    Back,
    StashChanged,
}

enum Mode {
    List,
    CreateInput,
    DropConfirm,
}

pub struct StashView {
    stashes: Vec<StashEntry>,
    cursor: usize,
    mode: Mode,
    input: String,
    message: String,
}

impl StashView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let stashes = repo.stashes()?;
        Ok(StashView {
            stashes,
            cursor: 0,
            mode: Mode::List,
            input: String::new(),
            message: "j/k 导航 · a apply · P pop · d drop · c 创建 · q 返回".into(),
        })
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match self.mode {
            Mode::List => self.handle_list(repo, c),
            Mode::CreateInput => self.handle_create(repo, c),
            Mode::DropConfirm => self.handle_drop_confirm(repo, c),
        }
    }

    fn handle_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        if self.stashes.is_empty() {
            if c == 'q' {
                return Ok(Action::Back);
            }
            if c == 'c' {
                self.mode = Mode::CreateInput;
                self.input.clear();
                self.message = "输入 stash 消息(Enter 创建 / Esc 取消)".into();
            }
            return Ok(Action::None);
        }

        match c {
            'q' => return Ok(Action::Back),
            'j' if self.cursor + 1 < self.stashes.len() => self.cursor += 1,
            'k' if self.cursor > 0 => self.cursor -= 1,
            'a' => {
                // apply
                if let Some(entry) = self.stashes.get(self.cursor) {
                    match repo.stash_apply(&entry.reff) {
                        Ok(()) => self.message = format!("已应用 {}", entry.reff),
                        Err(e) => self.message = format!("应用失败: {e}"),
                    }
                }
            }
            'P' => {
                // pop (大写)
                if let Some(entry) = self.stashes.get(self.cursor) {
                    match repo.stash_pop(&entry.reff) {
                        Ok(gitcore::PopResult::Clean) => {
                            self.message = format!("已弹出 {}", entry.reff);
                            self.stashes = repo.stashes()?;
                            if self.cursor >= self.stashes.len() && !self.stashes.is_empty() {
                                self.cursor = self.stashes.len() - 1;
                            }
                            return Ok(Action::StashChanged);
                        }
                        Ok(gitcore::PopResult::Conflict(files)) => {
                            self.message = format!("弹出冲突,{} 个文件需处理", files.len());
                        }
                        Err(e) => self.message = format!("弹出失败: {e}"),
                    }
                }
            }
            'd' => {
                if let Some(entry) = self.stashes.get(self.cursor) {
                    self.mode = Mode::DropConfirm;
                    self.message = format!("确认丢弃 {}?(y 确认 / 其他取消)", entry.reff);
                }
            }
            'c' => {
                self.mode = Mode::CreateInput;
                self.input.clear();
                self.message = "输入 stash 消息(Enter 创建 / Esc 取消)".into();
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_create(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            '\x1b' => {
                self.mode = Mode::List;
                self.message = "已取消创建".into();
            }
            '\n' => {
                let msg = if self.input.trim().is_empty() {
                    None
                } else {
                    Some(self.input.as_str())
                };
                match repo.stash_push(msg) {
                    Ok(()) => {
                        self.message = "已创建 stash".into();
                        self.mode = Mode::List;
                        self.stashes = repo.stashes()?;
                        return Ok(Action::StashChanged);
                    }
                    Err(e) => self.message = format!("创建失败: {e}"),
                }
            }
            '\x08' | '\x7f' => {
                self.input.pop();
            }
            c if !c.is_control() => {
                self.input.push(c);
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_drop_confirm(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        if c == 'y' || c == 'Y' {
            if let Some(entry) = self.stashes.get(self.cursor) {
                match repo.stash_drop(&entry.reff) {
                    Ok(()) => {
                        self.message = format!("已丢弃 {}", entry.reff);
                        self.stashes = repo.stashes()?;
                        if self.cursor >= self.stashes.len() && !self.stashes.is_empty() {
                            self.cursor = self.stashes.len() - 1;
                        }
                        self.mode = Mode::List;
                        return Ok(Action::StashChanged);
                    }
                    Err(e) => {
                        self.message = format!("丢弃失败: {e}");
                        self.mode = Mode::List;
                    }
                }
            }
        } else {
            self.mode = Mode::List;
            self.message = "已取消丢弃".into();
        }
        Ok(Action::None)
    }

    pub fn render(&self, f: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        f.render_widget(Paragraph::new(" Stash 视图 · 临时保存"), chunks[0]);

        match self.mode {
            Mode::List | Mode::DropConfirm => self.render_list(f, chunks[1]),
            Mode::CreateInput => self.render_create_input(f, chunks[1]),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.stashes.is_empty() {
            lines.push(Line::from("(无 stash,按 c 创建)"));
        } else {
            for (i, entry) in self.stashes.iter().enumerate() {
                let mut reff_style = Style::default().fg(Color::Yellow);
                let mut msg_style = Style::default();
                let mut branch_style = Style::default().fg(Color::Gray);
                if i == self.cursor {
                    reff_style = reff_style.add_modifier(Modifier::REVERSED);
                    msg_style = msg_style.add_modifier(Modifier::REVERSED);
                    branch_style = branch_style.add_modifier(Modifier::REVERSED);
                }

                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", entry.reff), reff_style),
                    Span::styled(&entry.message, msg_style),
                    Span::styled(format!(" ({})", entry.branch), branch_style),
                ]));
            }
        }
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" Stash 列表 ")),
            area,
        );
    }

    fn render_create_input(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let text = format!("消息(可为空): {}", self.input);
        f.render_widget(
            Paragraph::new(text).block(Block::bordered().title(" 创建 Stash ")),
            area,
        );
    }
}
