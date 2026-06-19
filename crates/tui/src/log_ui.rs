//! Log 视图:带分支拓扑图的提交历史列表 + 双栏详情(左侧提交信息/文件,右侧内容/diff)。

use gitcore::{Error, FileDiff, GraphRow, LineKind, LogOptions, Repo};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

pub enum Action {
    None,
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Files,
    Content,
}

#[derive(Clone, Copy)]
enum RowKind {
    Plain, // 提交信息文本
    HunkHeader,
    Context,
    Added,
    Removed,
}

struct DetailRow {
    text: String,
    kind: RowKind,
}

/// 详情态:某个 commit 的提交信息 + 按文件 diff(双栏浏览)。
struct Detail {
    sha: String,
    message_text: String,
    files: Vec<FileDiff>,
    sel: usize, // 0=提交信息;1.. = files[sel-1]
    focus: Focus,
    rows: Vec<DetailRow>, // 右栏当前项的行
    row_cursor: usize,
}

enum Mode {
    List,
    Detail(Detail),
}

pub struct LogView {
    rows: Vec<GraphRow>,
    cursor: usize, // List 模式:始终指向 commit 行
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
            Mode::Detail(_) => self.handle_detail(c),
        }
    }

    fn handle_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN => self.move_cursor(1),
            'k' | crate::keys::UP => self.move_cursor(-1),
            '\n' | '\r' => {
                if let Some(entry) = self.rows.get(self.cursor).and_then(|r| r.entry.as_ref()) {
                    let sha = entry.sha.clone();
                    let message_text = repo.commit_message(&sha).unwrap_or_default();
                    match repo.commit_files(&sha) {
                        Ok(files) => {
                            let rows = build_detail_rows(0, &message_text, &files);
                            self.mode = Mode::Detail(Detail {
                                sha,
                                message_text,
                                files,
                                sel: 0,
                                focus: Focus::Files,
                                rows,
                                row_cursor: 0,
                            });
                            self.message = "j/k 选项 · l/Enter 看内容 · h/q 返回列表".into();
                        }
                        Err(e) => self.message = format!("加载失败: {e}"),
                    }
                }
            }
            _ => {}
        }
        Ok(Action::None)
    }

    /// List 模式:在 commit 行之间移动,跳过纯图形连接行。
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
        let focus = match &self.mode {
            Mode::Detail(d) => d.focus,
            _ => return Ok(Action::None),
        };
        match focus {
            Focus::Files => match c {
                'q' | '\x1b' | 'h' | crate::keys::LEFT => {
                    self.mode = Mode::List;
                    self.message = "j/k 上下 · Enter 查看详情 · q/Esc 返回".into();
                }
                'j' | crate::keys::DOWN => self.detail_select(1),
                'k' | crate::keys::UP => self.detail_select(-1),
                'l' | crate::keys::RIGHT | '\n' | '\r' => {
                    if let Mode::Detail(d) = &mut self.mode {
                        if !d.rows.is_empty() {
                            d.focus = Focus::Content;
                        }
                    }
                }
                _ => {}
            },
            Focus::Content => {
                if let Mode::Detail(d) = &mut self.mode {
                    match c {
                        'q' | '\x1b' | 'h' | crate::keys::LEFT => d.focus = Focus::Files,
                        'j' | crate::keys::DOWN if d.row_cursor + 1 < d.rows.len() => {
                            d.row_cursor += 1
                        }
                        'k' | crate::keys::UP if d.row_cursor > 0 => d.row_cursor -= 1,
                        _ => {}
                    }
                }
            }
        }
        Ok(Action::None)
    }

    fn detail_select(&mut self, dir: isize) {
        if let Mode::Detail(d) = &mut self.mode {
            let n = d.files.len() + 1; // +1:提交信息项
            let next = (d.sel as isize + dir).clamp(0, n as isize - 1) as usize;
            if next != d.sel {
                d.sel = next;
                d.row_cursor = 0;
                d.rows = build_detail_rows(d.sel, &d.message_text, &d.files);
            }
        }
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
            Mode::Detail(d) => self.render_detail(f, chunks[1], d),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_list(&self, f: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        if self.rows.is_empty() {
            lines.push(Line::from("(无提交历史)"));
        } else {
            for (i, row) in self.rows.iter().enumerate() {
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

    fn render_detail(&self, f: &mut Frame, area: Rect, d: &Detail) {
        let panes =
            Layout::horizontal([Constraint::Percentage(28), Constraint::Min(10)]).split(area);

        // 左栏:提交信息 + 文件列表
        let fb = if d.focus == Focus::Files {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let mut items = vec![detail_item("提交信息", d.sel == 0, Color::White)];
        for (i, file) in d.files.iter().enumerate() {
            items.push(detail_item(&file.path, d.sel == i + 1, Color::Cyan));
        }
        f.render_widget(
            Paragraph::new(items)
                .block(Block::bordered().border_style(fb).title(" 文件 "))
                .scroll((
                    crate::scroll::follow(d.sel, area.height.saturating_sub(2)),
                    0,
                )),
            panes[0],
        );

        // 右栏:当前项内容(提交信息文本 或 文件 diff)
        let cb = if d.focus == Focus::Content {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let lines: Vec<Line> = d
            .rows
            .iter()
            .map(|r| {
                let color = match r.kind {
                    RowKind::Plain => Color::Reset,
                    RowKind::HunkHeader => Color::Magenta,
                    RowKind::Added => Color::Green,
                    RowKind::Removed => Color::Red,
                    RowKind::Context => Color::Gray,
                };
                Line::from(Span::styled(r.text.clone(), Style::default().fg(color)))
            })
            .collect();
        let title = format!(" 提交 {} ", d.sha);
        f.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().border_style(cb).title(title))
                .scroll((
                    crate::scroll::follow(d.row_cursor, area.height.saturating_sub(2)),
                    0,
                )),
            panes[1],
        );
    }
}

fn detail_item(text: &str, selected: bool, color: Color) -> Line<'static> {
    let mut style = Style::default().fg(color);
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    Line::from(Span::styled(text.to_string(), style))
}

/// 构造右栏行:`sel==0` 为提交信息文本,否则为 `files[sel-1]` 的 diff。
fn build_detail_rows(sel: usize, message_text: &str, files: &[FileDiff]) -> Vec<DetailRow> {
    if sel == 0 {
        return message_text
            .lines()
            .map(|l| DetailRow {
                text: l.to_string(),
                kind: RowKind::Plain,
            })
            .collect();
    }
    let Some(file) = files.get(sel - 1) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if file.binary {
        rows.push(DetailRow {
            text: "(二进制文件)".into(),
            kind: RowKind::Plain,
        });
        return rows;
    }
    for hunk in &file.hunks {
        let heading = if hunk.heading.is_empty() {
            String::new()
        } else {
            format!(" {}", hunk.heading)
        };
        rows.push(DetailRow {
            text: format!("@@ -{} +{} @@{}", hunk.old_start, hunk.new_start, heading),
            kind: RowKind::HunkHeader,
        });
        for line in &hunk.lines {
            let (prefix, kind) = match line.kind {
                LineKind::Context => (" ", RowKind::Context),
                LineKind::Added => ("+", RowKind::Added),
                LineKind::Removed => ("-", RowKind::Removed),
            };
            rows.push(DetailRow {
                text: format!("{prefix}{}", line.content),
                kind,
            });
        }
    }
    rows
}
