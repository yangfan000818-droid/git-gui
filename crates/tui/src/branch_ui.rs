//! Branch 视图:分支列表 + 创建/切换/删除。

use gitcore::{BranchInfo, Error, Repo};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Branch 视图的动作。
pub enum Action {
    None,
    Back,            // 返回 Status
    BranchesChanged, // 分支列表已变化
}

enum Mode {
    List,
    CreateInput,
    DeleteConfirm,
}

pub struct BranchView {
    branches: Vec<BranchInfo>,
    cursor: usize,
    mode: Mode,
    input: String,
    message: String,
}

impl BranchView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let branches = repo.branches()?;
        let cursor = branches.iter().position(|b| b.is_current).unwrap_or(0);
        Ok(BranchView {
            branches,
            cursor,
            mode: Mode::List,
            input: String::new(),
            message: "j/k 导航 · Enter 切换 · c 创建 · d 删除 · q/Esc 返回".into(),
        })
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match self.mode {
            Mode::List => self.handle_list(repo, c),
            Mode::CreateInput => self.handle_create(repo, c),
            Mode::DeleteConfirm => self.handle_delete_confirm(repo, c),
        }
    }

    fn handle_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN if self.cursor + 1 < self.branches.len() => self.cursor += 1,
            'k' | crate::keys::UP if self.cursor > 0 => self.cursor -= 1,
            '\n' => {
                // Enter: 切换分支
                if let Some(branch) = self.branches.get(self.cursor) {
                    if branch.is_current {
                        self.message = format!("已在分支 {}", branch.name);
                    } else {
                        match repo.switch_branch(&branch.name) {
                            Ok(()) => {
                                self.message = format!("已切换到 {}", branch.name);
                                self.branches = repo.branches()?;
                                self.cursor =
                                    self.branches.iter().position(|b| b.is_current).unwrap_or(0);
                                return Ok(Action::BranchesChanged);
                            }
                            Err(e) => self.message = format!("切换失败: {e}"),
                        }
                    }
                }
            }
            'c' => {
                self.mode = Mode::CreateInput;
                self.input.clear();
                self.message = "输入新分支名(Enter 创建 / Esc 取消)".into();
            }
            'd' => {
                if let Some(branch) = self.branches.get(self.cursor) {
                    if branch.is_current {
                        self.message = "不能删除当前分支".into();
                    } else {
                        self.mode = Mode::DeleteConfirm;
                        self.message = format!("确认删除 {}?(y 确认 / 其他取消)", branch.name);
                    }
                }
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_create(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            '\x1b' => {
                // Esc
                self.mode = Mode::List;
                self.message = "已取消创建".into();
            }
            '\n' => {
                // Enter
                if self.input.trim().is_empty() {
                    self.message = "分支名不能为空".into();
                    return Ok(Action::None);
                }
                match repo.create_branch(&self.input) {
                    Ok(()) => {
                        self.message = format!("已创建分支 {}", self.input);
                        self.mode = Mode::List;
                        self.branches = repo.branches()?;
                        self.cursor = self
                            .branches
                            .iter()
                            .position(|b| b.name == self.input)
                            .unwrap_or(self.cursor);
                        return Ok(Action::BranchesChanged);
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

    fn handle_delete_confirm(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        if c == 'y' || c == 'Y' {
            if let Some(branch) = self.branches.get(self.cursor) {
                match repo.delete_branch(&branch.name) {
                    Ok(()) => {
                        self.message = format!("已删除分支 {}", branch.name);
                        self.branches = repo.branches()?;
                        if self.cursor >= self.branches.len() && !self.branches.is_empty() {
                            self.cursor = self.branches.len() - 1;
                        }
                        self.mode = Mode::List;
                        return Ok(Action::BranchesChanged);
                    }
                    Err(e) => {
                        self.message = format!("删除失败: {e}");
                        self.mode = Mode::List;
                    }
                }
            }
        } else {
            self.mode = Mode::List;
            self.message = "已取消删除".into();
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

        f.render_widget(Paragraph::new(" Branch 视图 · 分支管理"), chunks[0]);

        match self.mode {
            Mode::List | Mode::DeleteConfirm => self.render_list(f, chunks[1]),
            Mode::CreateInput => self.render_create_input(f, chunks[1]),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.branches.is_empty() {
            lines.push(Line::from("(无分支)"));
        } else {
            for (i, branch) in self.branches.iter().enumerate() {
                let current_mark = if branch.is_current { "*" } else { " " };
                let name_text = format!("{} {}", current_mark, branch.name);

                let upstream_text = match &branch.upstream {
                    Some(up) => {
                        let track = match (branch.ahead, branch.behind) {
                            (0, 0) => String::new(),
                            (a, 0) => format!(" ↑{}", a),
                            (0, b) => format!(" ↓{}", b),
                            (a, b) => format!(" ↑{}↓{}", a, b),
                        };
                        format!(" → {}{}", up, track)
                    }
                    None => String::new(),
                };

                let mut name_style = Style::default();
                if branch.is_current {
                    name_style = name_style.fg(Color::Green);
                }
                if i == self.cursor {
                    name_style = name_style.add_modifier(Modifier::REVERSED);
                }

                let mut upstream_style = Style::default().fg(Color::Gray);
                if i == self.cursor {
                    upstream_style = upstream_style.add_modifier(Modifier::REVERSED);
                }

                lines.push(Line::from(vec![
                    Span::styled(name_text, name_style),
                    Span::styled(upstream_text, upstream_style),
                ]));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" 分支列表 "))
                .scroll((
                    crate::scroll::follow(self.cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }

    fn render_create_input(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let text = format!("新分支名: {}", self.input);
        f.render_widget(
            Paragraph::new(text).block(Block::bordered().title(" 创建分支 ")),
            area,
        );
    }
}
