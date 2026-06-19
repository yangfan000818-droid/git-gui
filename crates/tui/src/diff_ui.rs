//! Diff 视图:结构化 hunk 视图。
//! j/k 移动 · t 切未暂存/已暂存 · Space 选行(内容行)或暂存整 hunk/文件(结构行) · s 暂存选中行 · q/Esc 返回。

use std::collections::HashSet;

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

/// 一条可见行;`hunk_idx`/`line_idx` 同为 Some 时是可选择的内容行。
struct Row {
    text: String,
    kind: RowKind,
    file_idx: usize,
    hunk_idx: Option<usize>,
    line_idx: Option<usize>,
}

pub struct DiffView {
    staged: bool, // false=未暂存(工作区 vs 暂存区);true=已暂存(暂存区 vs HEAD)
    files: Vec<FileDiff>,
    rows: Vec<Row>,
    cursor: usize,
    // 行选择:限定在单个 hunk 内。active=(file_idx, hunk_idx),selected 为 hunk.lines 下标。
    active: Option<(usize, usize)>,
    selected: HashSet<usize>,
    message: String,
}

impl DiffView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let mut view = DiffView {
            staged: false,
            files: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            active: None,
            selected: HashSet::new(),
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
        self.active = None;
        self.selected.clear();
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
            format!("{mode}改动 · t 切暂存区 · Space 选行/整 hunk · s {act}选中行 · q/Esc 返回");
    }

    fn update_selection_message(&mut self) {
        let act = if self.staged {
            "取消暂存"
        } else {
            "暂存"
        };
        self.message = format!(
            "已选 {} 行 · s {act}选中行 · Space 继续选/取消选",
            self.selected.len()
        );
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        for (fi, file) in self.files.iter().enumerate() {
            rows.push(Row {
                text: format!("▾ {}", file.path),
                kind: RowKind::FileHeader,
                file_idx: fi,
                hunk_idx: None,
                line_idx: None,
            });
            if file.binary {
                rows.push(Row {
                    text: "    (二进制文件)".into(),
                    kind: RowKind::Context,
                    file_idx: fi,
                    hunk_idx: None,
                    line_idx: None,
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
                    line_idx: None,
                });
                for (li, line) in hunk.lines.iter().enumerate() {
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
                        line_idx: Some(li),
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
            ' ' => self.space_at_cursor(repo)?,
            's' => self.commit_selected(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    /// Space:内容行 → toggle 选中(限定单 hunk);结构行 → 暂存整 hunk/整文件。
    fn space_at_cursor(&mut self, repo: &Repo) -> Result<(), Error> {
        let (fi, hunk_idx, line_idx) = match self.rows.get(self.cursor) {
            Some(r) => (r.file_idx, r.hunk_idx, r.line_idx),
            None => return Ok(()),
        };
        match (hunk_idx, line_idx) {
            (Some(hi), Some(li)) => {
                // 切到另一个 hunk 选行时,先清空旧选择。
                if self.active != Some((fi, hi)) {
                    self.selected.clear();
                    self.active = Some((fi, hi));
                }
                if !self.selected.insert(li) {
                    self.selected.remove(&li);
                }
                if self.selected.is_empty() {
                    self.active = None;
                    self.set_default_message();
                } else {
                    self.update_selection_message();
                }
                Ok(())
            }
            _ => self.apply_whole_at_cursor(repo),
        }
    }

    /// 整 hunk(光标在 hunk header) / 整文件(光标在文件行)的暂存或取消。
    fn apply_whole_at_cursor(&mut self, repo: &Repo) -> Result<(), Error> {
        let (fi, hunk_idx) = match self.rows.get(self.cursor) {
            Some(r) => (r.file_idx, r.hunk_idx),
            None => return Ok(()),
        };
        let staged = self.staged;
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

    /// s:把选中的行暂存(未暂存视图)或取消暂存(已暂存视图)。
    fn commit_selected(&mut self, repo: &Repo) -> Result<(), Error> {
        let (fi, hi) = match self.active {
            Some(x) if !self.selected.is_empty() => x,
            _ => {
                self.message = "未选中行(在 +/- 行上按 Space 选择,再按 s)".into();
                return Ok(());
            }
        };
        let mut sel: Vec<usize> = self.selected.iter().copied().collect();
        sel.sort_unstable();
        let staged = self.staged;
        let n = sel.len();
        {
            let file = &self.files[fi];
            let hunk = &file.hunks[hi];
            if staged {
                repo.unstage_lines(file, hunk, &sel)?;
            } else {
                repo.stage_lines(file, hunk, &sel)?;
            }
        }
        self.reload(repo)?;
        let act = if staged { "取消暂存" } else { "暂存" };
        self.message = format!("已{act} {n} 行");
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
                let selected = self.is_selected(row);
                let cursor = i == self.cursor;

                let mut text_style = Style::default().fg(color);
                if row.kind == RowKind::FileHeader {
                    text_style = text_style.add_modifier(Modifier::BOLD);
                }
                if selected {
                    text_style = text_style.add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
                }
                if cursor {
                    text_style = text_style.add_modifier(Modifier::REVERSED);
                }
                let mut mark_style = Style::default().fg(Color::Yellow);
                if cursor {
                    mark_style = mark_style.add_modifier(Modifier::REVERSED);
                }
                let mark = if selected { "●" } else { " " };
                lines.push(Line::from(vec![
                    Span::styled(mark, mark_style),
                    Span::styled(row.text.clone(), text_style),
                ]));
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

    fn is_selected(&self, row: &Row) -> bool {
        match (row.hunk_idx, row.line_idx) {
            (Some(hi), Some(li)) => {
                self.active == Some((row.file_idx, hi)) && self.selected.contains(&li)
            }
            _ => false,
        }
    }
}
