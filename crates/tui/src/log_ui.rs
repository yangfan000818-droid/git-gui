//! Log 视图:提交历史列表 + 详情查看。

use gitcore::{Error, LogEntry, LogOptions, Repo};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

/// Log 视图的动作。
pub enum Action {
    None,
    Back, // 返回 Status
}

enum Mode {
    List,
    Detail { sha: String, content: String },
}

pub struct LogView {
    entries: Vec<LogEntry>,
    cursor: usize,
    mode: Mode,
    message: String,
}

impl LogView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let entries = repo.log(&LogOptions::default())?;
        Ok(LogView {
            entries,
            cursor: 0,
            mode: Mode::List,
            message: "j/k 上下 · Enter 查看详情 · q 返回".into(),
        })
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match &self.mode {
            Mode::List => self.handle_list(repo, c),
            Mode::Detail { .. } => self.handle_detail(c),
        }
    }

    fn handle_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' => return Ok(Action::Back),
            'j' if self.cursor + 1 < self.entries.len() => self.cursor += 1,
            'k' if self.cursor > 0 => self.cursor -= 1,
            '\n' | '\r' => {
                // Enter: 查看详情
                if let Some(entry) = self.entries.get(self.cursor) {
                    match repo.show_commit(&entry.sha) {
                        Ok(content) => {
                            self.mode = Mode::Detail {
                                sha: entry.sha.clone(),
                                content,
                            };
                            self.message = "q 返回列表".into();
                        }
                        Err(e) => self.message = format!("加载失败: {e}"),
                    }
                }
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_detail(&mut self, c: char) -> Result<Action, Error> {
        if c == 'q' {
            self.mode = Mode::List;
            self.message = "j/k 上下 · Enter 查看详情 · q 返回".into();
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

        f.render_widget(Paragraph::new(" Log 视图 · 提交历史"), chunks[0]);

        match &self.mode {
            Mode::List => self.render_list(f, chunks[1]),
            Mode::Detail { sha, content } => self.render_detail(f, chunks[1], sha, content),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.entries.is_empty() {
            lines.push(Line::from("(无提交历史)"));
        } else {
            for (i, entry) in self.entries.iter().enumerate() {
                let sha_span = Span::styled(&entry.sha, Style::default().fg(Color::Yellow));
                let msg_span = Span::raw(format!(" {}", entry.message));
                let author_span = Span::styled(
                    format!(" - {} ({})", entry.author, entry.date),
                    Style::default().fg(Color::Gray),
                );

                let mut style = Style::default();
                if i == self.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                let mut line_spans = vec![sha_span, msg_span, author_span];
                if i == self.cursor {
                    line_spans = line_spans.into_iter().map(|s| s.style(style)).collect();
                }

                lines.push(Line::from(line_spans));
            }
        }
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" 提交列表 ")),
            area,
        );
    }

    fn render_detail(&self, f: &mut Frame, area: ratatui::layout::Rect, sha: &str, content: &str) {
        let title = format!(" 提交详情: {} ", sha);
        f.render_widget(
            Paragraph::new(content)
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}
