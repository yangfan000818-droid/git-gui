//! Diff 视图:结构化 hunk 视图。
//! j/k 移动 · t 切未暂存/已暂存 · Space 暂存/取消光标所在 hunk(文件名行=整文件) · q/Esc 返回。

use gitcore::{Error, FileDiff, LineKind, Repo};
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
enum RowKind {
    FileHeader,
    HunkHeader,
    Context,
    Added,
    Removed,
}

/// 一条可见行;`hunk_idx` 为 None 表示文件标题行(操作=整文件)。
struct Row {
    text: String,
    kind: RowKind,
    file_idx: usize,
    hunk_idx: Option<usize>,
}

pub struct DiffView {
    staged: bool, // false=未暂存(工作区 vs 暂存区);true=已暂存(暂存区 vs HEAD)
    files: Vec<FileDiff>,
    rows: Vec<Row>,
    cursor: usize,
    message: String,
}

impl DiffView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let mut view = DiffView {
            staged: false,
            files: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            message: String::new(),
        };
        view.reload(repo)?;
        Ok(view)
    }

    fn reload(&mut self, repo: &Repo) -> Result<(), Error> {
        self.files = if self.staged {
            repo.staged_diff()?
        } else {
            repo.unstaged_diff()?
        };
        self.rebuild_rows();
        self.set_default_message();
        Ok(())
    }

    fn set_default_message(&mut self) {
        let mode = if self.staged {
            "已暂存"
        } else {
            "未暂存"
        };
        let act = if self.staged {
            "取消暂存"
        } else {
            "暂存"
        };
        self.message =
            format!("{mode}改动 · t 切暂存区 · Space {act}此 hunk(文件名行=整文件) · q/Esc 返回");
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (fi, file) in self.files.iter().enumerate() {
            rows.push(Row {
                text: format!("▾ {}", file.path),
                kind: RowKind::FileHeader,
                file_idx: fi,
                hunk_idx: None,
            });
            if file.binary {
                rows.push(Row {
                    text: "    (二进制文件)".into(),
                    kind: RowKind::Context,
                    file_idx: fi,
                    hunk_idx: None,
                });
                continue;
            }
            for (hi, hunk) in file.hunks.iter().enumerate() {
                let heading = if hunk.heading.is_empty() {
                    String::new()
                } else {
                    format!(" {}", hunk.heading)
                };
                rows.push(Row {
                    text: format!("@@ -{} +{} @@{}", hunk.old_start, hunk.new_start, heading),
                    kind: RowKind::HunkHeader,
                    file_idx: fi,
                    hunk_idx: Some(hi),
                });
                for line in &hunk.lines {
                    let (prefix, kind) = match line.kind {
                        LineKind::Context => (" ", RowKind::Context),
                        LineKind::Added => ("+", RowKind::Added),
                        LineKind::Removed => ("-", RowKind::Removed),
                    };
                    rows.push(Row {
                        text: format!("{prefix}{}", line.content),
                        kind,
                        file_idx: fi,
                        hunk_idx: Some(hi),
                    });
                }
            }
        }
        self.rows = rows;
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN if self.cursor + 1 < self.rows.len() => self.cursor += 1,
            'k' | crate::keys::UP if self.cursor > 0 => self.cursor -= 1,
            't' => {
                self.staged = !self.staged;
                self.cursor = 0;
                self.reload(repo)?;
            }
            ' ' => self.apply_at_cursor(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    fn apply_at_cursor(&mut self, repo: &Repo) -> Result<(), Error> {
        let (fi, hunk_idx) = match self.rows.get(self.cursor) {
            Some(r) => (r.file_idx, r.hunk_idx),
            None => return Ok(()),
        };
        let staged = self.staged;
        // file/hunk 借用限定在此块内,块结束后才 reload(&mut self)。
        let msg = {
            let file = &self.files[fi];
            let path = file.path.clone();
            match hunk_idx {
                Some(hi) => {
                    let hunk = &file.hunks[hi];
                    if staged {
                        repo.unstage_hunk(file, hunk)?;
                        format!("已取消暂存 {path} 的一个 hunk")
                    } else {
                        repo.stage_hunk(file, hunk)?;
                        format!("已暂存 {path} 的一个 hunk")
                    }
                }
                None => {
                    let p = std::path::PathBuf::from(&path);
                    if staged {
                        repo.unstage(&[&p])?;
                        format!("已取消暂存整个文件 {path}")
                    } else {
                        repo.stage(&[&p])?;
                        format!("已暂存整个文件 {path}")
                    }
                }
            }
        };
        self.reload(repo)?;
        self.message = msg;
        Ok(())
    }

    pub fn render(&self, f: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        let title = if self.staged {
            " Diff 视图 · 已暂存改动(暂存区 vs HEAD)"
        } else {
            " Diff 视图 · 未暂存改动(工作区 vs 暂存区)"
        };
        f.render_widget(Paragraph::new(title), chunks[0]);

        self.render_body(f, chunks[1]);

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_body(&self, f: &mut Frame, area: Rect) {
        let mut lines = Vec::new();
        if self.rows.is_empty() {
            let empty = if self.staged {
                "(暂存区无改动)"
            } else {
                "(工作区无未暂存改动)"
            };
            lines.push(Line::from(empty));
        } else {
            for (i, row) in self.rows.iter().enumerate() {
                let color = match row.kind {
                    RowKind::FileHeader => Color::Cyan,
                    RowKind::HunkHeader => Color::Magenta,
                    RowKind::Added => Color::Green,
                    RowKind::Removed => Color::Red,
                    RowKind::Context => Color::Gray,
                };
                let mut style = Style::default().fg(color);
                if row.kind == RowKind::FileHeader {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if i == self.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                lines.push(Line::from(Span::styled(row.text.clone(), style)));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Diff "))
                .scroll((
                    crate::scroll::follow(self.cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }
}
