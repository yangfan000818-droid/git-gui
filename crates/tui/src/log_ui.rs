//! Log 视图:带分支拓扑图的提交历史列表 + 详情查看。

use gitcore::{Error, GraphRow, LogOptions, Repo};
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
    Detail {
        sha: String,
        content: String,
        scroll: u16,
    },
}

pub struct LogView {
    rows: Vec<GraphRow>,
    cursor: usize, // 始终指向 entry.is_some() 的 commit 行
    mode: Mode,
    message: String,
}

impl LogView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let rows = repo.log_graph(&LogOptions::default())?;
        let cursor = rows.iter().position(|r| r.entry.is_some()).unwrap_or(0);
        Ok(LogView {
            rows,
            cursor,
            mode: Mode::List,
            message: "j/k 上下 · Enter 查看详情 · q/Esc 返回".into(),
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
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN => self.move_cursor(1),
            'k' | crate::keys::UP => self.move_cursor(-1),
            '\n' | '\r' => {
                // Enter: 查看当前 commit 行的详情
                if let Some(entry) = self.rows.get(self.cursor).and_then(|r| r.entry.as_ref()) {
                    let sha = entry.sha.clone();
                    match repo.show_commit(&sha) {
                        Ok(content) => {
                            self.mode = Mode::Detail {
                                sha,
                                content,
                                scroll: 0,
                            };
                            self.message = "j/k 滚动 · q/Esc 返回列表".into();
                        }
                        Err(e) => self.message = format!("加载失败: {e}"),
                    }
                }
            }
            _ => {}
        }
        Ok(Action::None)
    }

    /// 在 commit 行之间移动光标,跳过纯图形连接行;到边界则保持不动。
    fn move_cursor(&mut self, dir: isize) {
        let mut i = self.cursor as isize;
        loop {
            i += dir;
            if i < 0 || i >= self.rows.len() as isize {
                return;
            }
            if self.rows[i as usize].entry.is_some() {
                self.cursor = i as usize;
                return;
            }
        }
    }

    fn handle_detail(&mut self, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => {
                self.mode = Mode::List;
                self.message = "j/k 上下 · Enter 查看详情 · q/Esc 返回".into();
            }
            'j' | 'k' | crate::keys::DOWN | crate::keys::UP => {
                if let Mode::Detail {
                    content, scroll, ..
                } = &mut self.mode
                {
                    let max = content.lines().count().saturating_sub(1) as u16;
                    *scroll = if c == 'j' || c == crate::keys::DOWN {
                        (*scroll + 1).min(max)
                    } else {
                        scroll.saturating_sub(1)
                    };
                }
            }
            _ => {}
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

        f.render_widget(Paragraph::new(" Log 视图 · 提交历史(含分支图)"), chunks[0]);

        match &self.mode {
            Mode::List => self.render_list(f, chunks[1]),
            Mode::Detail {
                sha,
                content,
                scroll,
            } => self.render_detail(f, chunks[1], sha, content, *scroll),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_list(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.rows.is_empty() {
            lines.push(Line::from("(无提交历史)"));
        } else {
            for (i, row) in self.rows.iter().enumerate() {
                // 图形列固定蓝色(不随光标反色,保持拓扑线清晰)。
                let mut spans = vec![Span::styled(
                    row.graph.clone(),
                    Style::default().fg(Color::Blue),
                )];
                if let Some(e) = &row.entry {
                    let mut data = vec![
                        Span::styled(e.sha.clone(), Style::default().fg(Color::Yellow)),
                        Span::raw(format!(" {}", e.message)),
                        Span::styled(
                            format!(" - {} ({})", e.author, e.date),
                            Style::default().fg(Color::Gray),
                        ),
                    ];
                    if i == self.cursor {
                        let rev = Style::default().add_modifier(Modifier::REVERSED);
                        data = data.into_iter().map(|s| s.style(rev)).collect();
                    }
                    spans.extend(data);
                }
                lines.push(Line::from(spans));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" 提交列表 "))
                .scroll((
                    crate::scroll::follow(self.cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }

    fn render_detail(
        &self,
        f: &mut Frame,
        area: ratatui::layout::Rect,
        sha: &str,
        content: &str,
        scroll: u16,
    ) {
        let title = format!(" 提交详情: {} ", sha);
        f.render_widget(
            Paragraph::new(content)
                .block(Block::bordered().title(title))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            area,
        );
    }
}
