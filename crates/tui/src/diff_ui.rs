//! Diff 视图:左侧文件目录树 + 右侧单文件 hunk diff 的双栏布局。
//!
//! 左栏(文件树):j/k 移动 · l/h 展开折叠目录或进/出 diff · Space 暂存整文件/整目录 · t 切暂存区 · q 返回。
//! 右栏(diff):j/k 移动 · Space 选行或暂存整 hunk · s 暂存选中行 · h/q 回文件树。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gitcore::{Error, FileDiff, LineKind, Repo};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::filetree::{self, RowKind as TreeKind};

pub enum Action {
    None,
    Back,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Files,
    Diff,
}

#[derive(Clone, Copy, PartialEq)]
enum RowKind {
    HunkHeader,
    Context,
    Added,
    Removed,
}

/// 右栏一行(当前文件的 hunk 头或内容行)。
struct Row {
    text: String,
    kind: RowKind,
    hunk_idx: usize,
    line_idx: Option<usize>,
}

pub struct DiffView {
    staged: bool,
    files: Vec<FileDiff>,
    collapsed: HashSet<PathBuf>,
    tree: Vec<filetree::TreeRow>,
    tree_cursor: usize,
    focus: Focus,
    // 右栏:当前文件的行投影
    rows: Vec<Row>,
    row_cursor: usize,
    // 行选择(限当前文件的单个 hunk)
    active_hunk: Option<usize>,
    selected: HashSet<usize>,
    message: String,
}

impl DiffView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let mut view = DiffView {
            staged: false,
            files: Vec::new(),
            collapsed: HashSet::new(),
            tree: Vec::new(),
            tree_cursor: 0,
            focus: Focus::Files,
            rows: Vec::new(),
            row_cursor: 0,
            active_hunk: None,
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
        self.rebuild_tree();
        self.active_hunk = None;
        self.selected.clear();
        self.rebuild_right_rows();
        if self.rows.is_empty() {
            self.focus = Focus::Files;
        }
        self.set_default_message();
        Ok(())
    }

    fn rebuild_tree(&mut self) {
        let paths: Vec<String> = self.files.iter().map(|f| f.path.clone()).collect();
        self.tree = filetree::build_rows(&paths, &self.collapsed);
        if self.tree_cursor >= self.tree.len() {
            self.tree_cursor = self.tree.len().saturating_sub(1);
        }
    }

    fn current_file_index(&self) -> Option<usize> {
        match self.tree.get(self.tree_cursor)?.kind {
            TreeKind::File { index } => Some(index),
            TreeKind::Dir { .. } => None,
        }
    }

    fn rebuild_right_rows(&mut self) {
        let mut rows = Vec::new();
        if let Some(idx) = self.current_file_index() {
            if let Some(file) = self.files.get(idx) {
                for (hi, hunk) in file.hunks.iter().enumerate() {
                    let heading = if hunk.heading.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", hunk.heading)
                    };
                    rows.push(Row {
                        text: format!("@@ -{} +{} @@{}", hunk.old_start, hunk.new_start, heading),
                        kind: RowKind::HunkHeader,
                        hunk_idx: hi,
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
                            hunk_idx: hi,
                            line_idx: Some(li),
                        });
                    }
                }
            }
        }
        self.rows = rows;
        if self.row_cursor >= self.rows.len() {
            self.row_cursor = self.rows.len().saturating_sub(1);
        }
    }

    fn set_default_message(&mut self) {
        let mode = if self.staged {
            "已暂存"
        } else {
            "未暂存"
        };
        self.message = match self.focus {
            Focus::Files => format!(
                "{mode} · j/k 选 · l/h 展开折叠或进 diff · Space 暂存文件/目录 · t 切暂存区 · q 返回"
            ),
            Focus::Diff => {
                let act = if self.staged { "取消暂存" } else { "暂存" };
                format!("{mode} · j/k 移动 · Space 选行/{act}整 hunk · s {act}选中行 · h/q 回文件树")
            }
        };
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

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match self.focus {
            Focus::Files => self.handle_files(repo, c),
            Focus::Diff => self.handle_diff(repo, c),
        }
    }

    fn on_dir(&self) -> bool {
        matches!(
            self.tree.get(self.tree_cursor).map(|r| &r.kind),
            Some(TreeKind::Dir { .. })
        )
    }

    fn handle_files(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN => self.move_tree(1),
            'k' | crate::keys::UP => self.move_tree(-1),
            'l' | crate::keys::RIGHT => {
                if self.on_dir() {
                    self.set_expand(true);
                } else {
                    self.enter_diff();
                }
            }
            'h' | crate::keys::LEFT => {
                if self.on_dir() {
                    self.set_expand(false);
                }
            }
            '\n' | '\r' => {
                if self.on_dir() {
                    self.toggle_expand();
                } else {
                    self.enter_diff();
                }
            }
            't' => self.toggle_staged(repo)?,
            ' ' => self.space_in_files(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    fn enter_diff(&mut self) {
        if !self.rows.is_empty() {
            self.focus = Focus::Diff;
            self.set_default_message();
        }
    }

    fn handle_diff(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' | 'h' | crate::keys::LEFT => {
                self.focus = Focus::Files;
                self.set_default_message();
            }
            'j' | crate::keys::DOWN if self.row_cursor + 1 < self.rows.len() => {
                self.row_cursor += 1
            }
            'k' | crate::keys::UP if self.row_cursor > 0 => self.row_cursor -= 1,
            ' ' => self.space_in_diff(repo)?,
            's' => self.commit_selected(repo)?,
            't' => self.toggle_staged(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    fn toggle_staged(&mut self, repo: &Repo) -> Result<(), Error> {
        self.staged = !self.staged;
        self.tree_cursor = 0;
        self.row_cursor = 0;
        self.focus = Focus::Files;
        self.reload(repo)
    }

    fn move_tree(&mut self, dir: isize) {
        let n = self.tree.len();
        if n == 0 {
            return;
        }
        let next = (self.tree_cursor as isize + dir).clamp(0, n as isize - 1) as usize;
        if next != self.tree_cursor {
            self.tree_cursor = next;
            self.row_cursor = 0;
            self.active_hunk = None;
            self.selected.clear();
            self.rebuild_right_rows();
            self.set_default_message();
        }
    }

    fn set_expand(&mut self, want: bool) {
        let path = match self.tree.get(self.tree_cursor) {
            Some(r) if matches!(r.kind, TreeKind::Dir { .. }) => r.path.clone(),
            _ => return,
        };
        if want {
            self.collapsed.remove(&path);
        } else {
            self.collapsed.insert(path);
        }
        self.rebuild_tree();
        self.rebuild_right_rows();
    }

    fn toggle_expand(&mut self) {
        let path = match self.tree.get(self.tree_cursor) {
            Some(r) if matches!(r.kind, TreeKind::Dir { .. }) => r.path.clone(),
            _ => return,
        };
        if self.collapsed.contains(&path) {
            self.collapsed.remove(&path);
        } else {
            self.collapsed.insert(path);
        }
        self.rebuild_tree();
    }

    /// 文件树上 Space:File → 暂存整文件;Dir → 暂存该目录下所有文件。
    fn space_in_files(&mut self, repo: &Repo) -> Result<(), Error> {
        let (is_dir, path) = match self.tree.get(self.tree_cursor) {
            Some(r) => (matches!(r.kind, TreeKind::Dir { .. }), r.path.clone()),
            None => return Ok(()),
        };
        let staged = self.staged;
        if is_dir {
            let paths: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|f| Path::new(&f.path).starts_with(&path))
                .map(|f| PathBuf::from(&f.path))
                .collect();
            if paths.is_empty() {
                return Ok(());
            }
            let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
            let n = refs.len();
            if staged {
                repo.unstage(&refs)?;
            } else {
                repo.stage(&refs)?;
            }
            self.reload(repo)?;
            self.message = format!(
                "已{}目录 {}/ ({} 个文件)",
                if staged { "取消暂存" } else { "暂存" },
                path.display(),
                n
            );
        } else {
            let p = PathBuf::from(&path);
            if staged {
                repo.unstage(&[p.as_path()])?;
            } else {
                repo.stage(&[p.as_path()])?;
            }
            self.reload(repo)?;
            self.message = format!(
                "已{}文件 {}",
                if staged { "取消暂存" } else { "暂存" },
                path.display()
            );
        }
        Ok(())
    }

    /// diff 右栏 Space:内容行 → 选行;hunk 头 → 暂存整 hunk。
    fn space_in_diff(&mut self, repo: &Repo) -> Result<(), Error> {
        let (hunk_idx, line_idx) = match self.rows.get(self.row_cursor) {
            Some(r) => (r.hunk_idx, r.line_idx),
            None => return Ok(()),
        };
        match line_idx {
            Some(li) => {
                if self.active_hunk != Some(hunk_idx) {
                    self.selected.clear();
                    self.active_hunk = Some(hunk_idx);
                }
                if !self.selected.insert(li) {
                    self.selected.remove(&li);
                }
                if self.selected.is_empty() {
                    self.active_hunk = None;
                    self.set_default_message();
                } else {
                    self.update_selection_message();
                }
                Ok(())
            }
            None => self.apply_hunk(repo, hunk_idx),
        }
    }

    fn apply_hunk(&mut self, repo: &Repo, hunk_idx: usize) -> Result<(), Error> {
        let idx = match self.current_file_index() {
            Some(i) => i,
            None => return Ok(()),
        };
        let staged = self.staged;
        let path = self.files[idx].path.clone();
        {
            let file = &self.files[idx];
            let hunk = &file.hunks[hunk_idx];
            if staged {
                repo.unstage_hunk(file, hunk)?;
            } else {
                repo.stage_hunk(file, hunk)?;
            }
        }
        self.reload(repo)?;
        self.message = format!(
            "已{} {path} 的一个 hunk",
            if staged { "取消暂存" } else { "暂存" }
        );
        Ok(())
    }

    fn commit_selected(&mut self, repo: &Repo) -> Result<(), Error> {
        let hunk_idx = match self.active_hunk {
            Some(h) if !self.selected.is_empty() => h,
            _ => {
                self.message = "未选中行(在 +/- 行上按 Space 选择,再按 s)".into();
                return Ok(());
            }
        };
        let idx = match self.current_file_index() {
            Some(i) => i,
            None => return Ok(()),
        };
        let mut sel: Vec<usize> = self.selected.iter().copied().collect();
        sel.sort_unstable();
        let staged = self.staged;
        let n = sel.len();
        {
            let file = &self.files[idx];
            let hunk = &file.hunks[hunk_idx];
            if staged {
                repo.unstage_lines(file, hunk, &sel)?;
            } else {
                repo.stage_lines(file, hunk, &sel)?;
            }
        }
        self.reload(repo)?;
        self.message = format!("已{} {n} 行", if staged { "取消暂存" } else { "暂存" });
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

        let panes =
            Layout::horizontal([Constraint::Percentage(32), Constraint::Min(10)]).split(chunks[1]);
        self.render_tree(f, panes[0]);
        self.render_diff(f, panes[1]);

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_tree(&self, f: &mut Frame, area: Rect) {
        let border = if self.focus == Focus::Files {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let mut lines = Vec::new();
        if self.tree.is_empty() {
            let empty = if self.staged {
                "(暂存区无改动)"
            } else {
                "(无未暂存改动)"
            };
            lines.push(Line::from(empty));
        } else {
            for (i, row) in self.tree.iter().enumerate() {
                let indent = "  ".repeat(row.depth);
                let (prefix, color, label) = match row.kind {
                    TreeKind::Dir { expanded } => (
                        if expanded { "▾ " } else { "▸ " },
                        Color::Cyan,
                        format!("{}/", row.name),
                    ),
                    TreeKind::File { .. } => ("  ", Color::Reset, row.name.clone()),
                };
                let mut style = Style::default().fg(color);
                if i == self.tree_cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                lines.push(Line::from(vec![
                    Span::styled(indent, style),
                    Span::styled(prefix, style),
                    Span::styled(label, style),
                ]));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().border_style(border).title(" 文件 "))
                .scroll((
                    crate::scroll::follow(self.tree_cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }

    fn render_diff(&self, f: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Diff;
        let border = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let title = self
            .current_file_index()
            .and_then(|i| self.files.get(i))
            .map(|file| format!(" {} ", file.path))
            .unwrap_or_else(|| " diff ".into());

        let mut lines = Vec::new();
        if self.rows.is_empty() {
            let binary = self
                .current_file_index()
                .and_then(|i| self.files.get(i))
                .map(|file| file.binary)
                .unwrap_or(false);
            lines.push(Line::from(if binary {
                "(二进制文件)"
            } else {
                "(选择左侧文件查看改动)"
            }));
        } else {
            for (i, row) in self.rows.iter().enumerate() {
                let color = match row.kind {
                    RowKind::HunkHeader => Color::Magenta,
                    RowKind::Added => Color::Green,
                    RowKind::Removed => Color::Red,
                    RowKind::Context => Color::Gray,
                };
                let selected = self.is_selected(row);
                let cursor = focused && i == self.row_cursor;

                let mut text_style = Style::default().fg(color);
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
                .block(Block::bordered().border_style(border).title(title))
                .scroll((
                    crate::scroll::follow(self.row_cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }

    fn is_selected(&self, row: &Row) -> bool {
        match row.line_idx {
            Some(li) => self.active_hunk == Some(row.hunk_idx) && self.selected.contains(&li),
            None => false,
        }
    }
}
