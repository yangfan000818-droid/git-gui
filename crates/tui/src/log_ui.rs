//! Log 视图:带分支拓扑图的提交历史列表 + 双栏详情。
//! 详情左栏 = 「提交信息」+ 改动文件目录树,右栏 = 当前项内容(提交信息全文 或 该文件 diff)。

use std::collections::HashSet;
use std::path::PathBuf;

use gitcore::{Error, FileDiff, GraphRow, LineKind, LogOptions, Repo};
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

/// 详情态:左栏 cursor 0=提交信息,1.. = tree[cursor-1];右栏随当前项变化。
struct Detail {
    sha: String,
    full_sha: String,
    message_text: String,
    files: Vec<FileDiff>,
    collapsed: HashSet<PathBuf>,
    tree: Vec<filetree::TreeRow>,
    cursor: usize,
    focus: Focus,
    rows: Vec<DetailRow>,
    row_cursor: usize,
}

impl Detail {
    fn on_dir(&self) -> bool {
        self.cursor > 0
            && matches!(
                self.tree.get(self.cursor - 1).map(|r| &r.kind),
                Some(TreeKind::Dir { .. })
            )
    }

    fn sel_file_index(&self) -> Option<usize> {
        if self.cursor == 0 {
            return None;
        }
        match self.tree.get(self.cursor - 1)?.kind {
            TreeKind::File { index } => Some(index),
            TreeKind::Dir { .. } => None,
        }
    }

    fn dir_path(&self) -> Option<PathBuf> {
        if self.cursor == 0 {
            return None;
        }
        match self.tree.get(self.cursor - 1) {
            Some(r) if matches!(r.kind, TreeKind::Dir { .. }) => Some(r.path.clone()),
            _ => None,
        }
    }

    fn rebuild_tree(&mut self) {
        let paths: Vec<String> = self.files.iter().map(|f| f.path.clone()).collect();
        self.tree = filetree::build_rows(&paths, &self.collapsed);
        if self.cursor > self.tree.len() {
            self.cursor = self.tree.len();
        }
    }

    fn rebuild_rows(&mut self) {
        self.rows = if self.cursor == 0 {
            self.message_text
                .lines()
                .map(|l| DetailRow {
                    text: l.to_string(),
                    kind: RowKind::Plain,
                })
                .collect()
        } else if let Some(idx) = self.sel_file_index() {
            file_diff_rows(&self.files[idx])
        } else {
            Vec::new()
        };
        if self.row_cursor >= self.rows.len() {
            self.row_cursor = self.rows.len().saturating_sub(1);
        }
    }
}

enum Mode {
    List,
    Detail(Box<Detail>),
}

pub struct LogView {
    rows: Vec<GraphRow>,
    cursor: usize,
    max_count: usize, // 当前加载上限;滚到底递增重拉(保证 graph 整体一致)
    has_more: bool,
    mode: Mode,
    message: String,
}

impl LogView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let max_count = 50;
        let rows = repo.log_graph(&LogOptions {
            max_count,
            branch: None,
        })?;
        let has_more = count_commits(&rows) >= max_count;
        let cursor = rows.iter().position(|r| r.entry.is_some()).unwrap_or(0);
        Ok(LogView {
            rows,
            cursor,
            max_count,
            has_more,
            mode: Mode::List,
            message: "j/k 上下 · Enter 详情 · y 复制哈希 · q 返回".into(),
        })
    }

    /// 滚到接近底部时调用:加大上限重拉(graph 整体重算,前面的行不变,光标保持)。
    fn load_more(&mut self, repo: &Repo) -> Result<(), Error> {
        if !self.has_more {
            return Ok(());
        }
        self.max_count += 50;
        let rows = repo.log_graph(&LogOptions {
            max_count: self.max_count,
            branch: None,
        })?;
        self.has_more = count_commits(&rows) >= self.max_count;
        self.rows = rows;
        self.message = format!("已加载 {} 条提交", count_commits(&self.rows));
        Ok(())
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
            'j' | crate::keys::DOWN => {
                self.move_cursor(1);
                if self.has_more && self.cursor + 5 >= self.rows.len() {
                    self.load_more(repo)?;
                }
            }
            'k' | crate::keys::UP => self.move_cursor(-1),
            'y' => self.copy_current_sha(),
            '\n' | '\r' => {
                if let Some(entry) = self.rows.get(self.cursor).and_then(|r| r.entry.as_ref()) {
                    let sha = entry.sha.clone();
                    let full_sha = entry.full_sha.clone();
                    let message_text = repo.commit_message(&sha).unwrap_or_default();
                    match repo.commit_files(&sha) {
                        Ok(files) => {
                            let collapsed = HashSet::new();
                            let paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
                            let tree = filetree::build_rows(&paths, &collapsed);
                            let mut detail = Detail {
                                sha,
                                full_sha,
                                message_text,
                                files,
                                collapsed,
                                tree,
                                cursor: 0,
                                focus: Focus::Files,
                                rows: Vec::new(),
                                row_cursor: 0,
                            };
                            detail.rebuild_rows();
                            self.mode = Mode::Detail(Box::new(detail));
                            self.message =
                                "j/k 选 · l 看内容/展开 · h 折叠 · y 复制哈希 · q 返回".into();
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

    fn copy_current_sha(&mut self) {
        if let Some(entry) = self.rows.get(self.cursor).and_then(|r| r.entry.as_ref()) {
            let full = entry.full_sha.clone();
            self.message = copy_sha_message(&full);
        }
    }

    fn copy_detail_sha(&mut self) {
        let full = match &self.mode {
            Mode::Detail(d) => d.full_sha.clone(),
            _ => return,
        };
        self.message = copy_sha_message(&full);
    }

    fn handle_detail(&mut self, c: char) -> Result<Action, Error> {
        if c == 'y' {
            self.copy_detail_sha();
            return Ok(Action::None);
        }
        let focus = match &self.mode {
            Mode::Detail(d) => d.focus,
            _ => return Ok(Action::None),
        };
        match focus {
            Focus::Files => {
                let on_dir = matches!(&self.mode, Mode::Detail(d) if d.on_dir());
                match c {
                    'q' | '\x1b' => {
                        self.mode = Mode::List;
                        self.message = "j/k 上下 · Enter 查看详情 · q/Esc 返回".into();
                    }
                    'j' | crate::keys::DOWN => self.detail_move(1),
                    'k' | crate::keys::UP => self.detail_move(-1),
                    'l' | crate::keys::RIGHT => {
                        if on_dir {
                            self.detail_set_expand(true);
                        } else {
                            self.detail_enter_content();
                        }
                    }
                    'h' | crate::keys::LEFT => {
                        if on_dir {
                            self.detail_set_expand(false);
                        }
                    }
                    '\n' | '\r' => {
                        if on_dir {
                            self.detail_toggle_expand();
                        } else {
                            self.detail_enter_content();
                        }
                    }
                    _ => {}
                }
            }
            Focus::Content => {
                let mut back = false;
                if let Mode::Detail(d) = &mut self.mode {
                    match c {
                        'q' | '\x1b' | 'h' | crate::keys::LEFT => {
                            d.focus = Focus::Files;
                            back = true;
                        }
                        'j' | crate::keys::DOWN if d.row_cursor + 1 < d.rows.len() => {
                            d.row_cursor += 1
                        }
                        'k' | crate::keys::UP if d.row_cursor > 0 => d.row_cursor -= 1,
                        _ => {}
                    }
                }
                if back {
                    self.message = "j/k 选 · l 看内容/展开 · h 折叠 · q 返回列表".into();
                }
            }
        }
        Ok(Action::None)
    }

    fn detail_move(&mut self, dir: isize) {
        if let Mode::Detail(d) = &mut self.mode {
            let max = d.tree.len() as isize; // cursor 范围 0..=tree.len()
            let next = (d.cursor as isize + dir).clamp(0, max) as usize;
            if next != d.cursor {
                d.cursor = next;
                d.row_cursor = 0;
                d.rebuild_rows();
            }
        }
    }

    fn detail_enter_content(&mut self) {
        let entered = if let Mode::Detail(d) = &mut self.mode {
            if d.rows.is_empty() {
                false
            } else {
                d.focus = Focus::Content;
                true
            }
        } else {
            false
        };
        if entered {
            self.message = "j/k 滚动内容 · h/q 回左栏".into();
        }
    }

    fn detail_set_expand(&mut self, want: bool) {
        if let Mode::Detail(d) = &mut self.mode {
            let Some(path) = d.dir_path() else {
                return;
            };
            if want {
                d.collapsed.remove(&path);
            } else {
                d.collapsed.insert(path);
            }
            d.rebuild_tree();
            d.rebuild_rows();
        }
    }

    fn detail_toggle_expand(&mut self) {
        if let Mode::Detail(d) = &mut self.mode {
            let Some(path) = d.dir_path() else {
                return;
            };
            if d.collapsed.contains(&path) {
                d.collapsed.remove(&path);
            } else {
                d.collapsed.insert(path);
            }
            d.rebuild_tree();
            d.rebuild_rows();
        }
    }

    /// 滚轮:详情右栏(Content)纯滚动;列表 / 详情左栏移选中。
    pub fn scroll_wheel(&mut self, delta: i32) {
        if let Mode::Detail(d) = &mut self.mode {
            if d.focus == Focus::Content {
                if delta > 0 {
                    let max = d.rows.len().saturating_sub(1);
                    if d.row_cursor < max {
                        d.row_cursor += 1;
                    }
                } else {
                    d.row_cursor = d.row_cursor.saturating_sub(1);
                }
                return;
            }
        }
        let dir = if delta > 0 { 1 } else { -1 };
        match &self.mode {
            Mode::List => self.move_cursor(dir),
            Mode::Detail(_) => self.detail_move(dir),
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
            Layout::horizontal([Constraint::Percentage(32), Constraint::Min(10)]).split(area);

        // 左栏:提交信息 + 文件目录树
        let fb = if d.focus == Focus::Files {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        let mut items = Vec::new();
        let mut info_style = Style::default().fg(Color::White);
        if d.cursor == 0 {
            info_style = info_style.add_modifier(Modifier::REVERSED);
        }
        items.push(Line::from(Span::styled("提交信息", info_style)));
        for (i, row) in d.tree.iter().enumerate() {
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
            if d.cursor == i + 1 {
                style = style.add_modifier(Modifier::REVERSED);
            }
            items.push(Line::from(vec![
                Span::styled(indent, style),
                Span::styled(prefix, style),
                Span::styled(label, style),
            ]));
        }
        f.render_widget(
            Paragraph::new(items)
                .block(Block::bordered().border_style(fb).title(" 文件 "))
                .scroll((
                    crate::scroll::follow(d.cursor, area.height.saturating_sub(2)),
                    0,
                )),
            panes[0],
        );

        // 右栏:当前项内容
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
                .scroll((d.row_cursor as u16, 0)),
            panes[1],
        );
    }
}

fn count_commits(rows: &[GraphRow]) -> usize {
    rows.iter().filter(|r| r.entry.is_some()).count()
}

/// 复制 sha 到剪贴板,返回给用户的提示消息。
fn copy_sha_message(full: &str) -> String {
    if crate::clipboard::copy(full) {
        format!("已复制哈希 {}", &full[..full.len().min(12)])
    } else {
        "复制失败:未找到剪贴板命令(pbcopy/wl-copy/xclip)".to_string()
    }
}

/// 把一个文件的 diff 渲染成右栏行(着色)。
fn file_diff_rows(file: &FileDiff) -> Vec<DetailRow> {
    if file.binary {
        return vec![DetailRow {
            text: "(二进制文件)".into(),
            kind: RowKind::Plain,
        }];
    }
    let mut rows = Vec::new();
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
