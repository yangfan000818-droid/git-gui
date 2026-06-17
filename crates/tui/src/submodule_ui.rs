//! Submodule 视图:列出子仓库,Enter 切换进入。

use gitcore::{Submodule, SubmoduleStatus};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::path::PathBuf;

/// Submodule 视图的动作。
pub enum Action {
    None,
    Back,              // 返回 Status
    SwitchTo(PathBuf), // 切换到子仓库
}

pub struct SubmoduleView {
    submodules: Vec<Submodule>,
    cursor: usize,
    message: String,
}

impl SubmoduleView {
    pub fn load(submodules: Vec<Submodule>) -> Self {
        SubmoduleView {
            submodules,
            cursor: 0,
            message: "j/k 上下 · Enter 切换 · q 返回".into(),
        }
    }

    pub fn handle_key(&mut self, c: char) -> Action {
        match c {
            'q' => return Action::Back,
            'j' if self.cursor + 1 < self.submodules.len() => self.cursor += 1,
            'k' if self.cursor > 0 => self.cursor -= 1,
            '\n' | '\r' => {
                // Enter: 切换到子仓库
                if let Some(sub) = self.submodules.get(self.cursor) {
                    if sub.status == SubmoduleStatus::Uninitialized {
                        self.message = "子仓库未初始化,请先执行 git submodule update --init".into();
                    } else {
                        return Action::SwitchTo(sub.path.clone());
                    }
                }
            }
            _ => {}
        }
        Action::None
    }

    pub fn render(&self, f: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        f.render_widget(Paragraph::new(" Submodule 视图 · 子仓库列表"), chunks[0]);

        let mut lines = Vec::new();
        if self.submodules.is_empty() {
            lines.push(Line::from("(无子仓库)"));
        } else {
            for (i, sub) in self.submodules.iter().enumerate() {
                let (status_icon, status_color) = match sub.status {
                    SubmoduleStatus::Clean => ("✓", Color::Green),
                    SubmoduleStatus::Dirty => ("●", Color::Yellow),
                    SubmoduleStatus::Detached => ("⚠", Color::Red),
                    SubmoduleStatus::Uninitialized => ("?", Color::Gray),
                };

                let mut style = Style::default();
                if i == self.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                let line = Line::from(vec![
                    Span::styled(status_icon, Style::default().fg(status_color)),
                    Span::styled(format!(" {}", sub.name), style),
                ]);
                lines.push(line);
            }
        }

        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" 子仓库列表 ")),
            chunks[1],
        );

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }
}
