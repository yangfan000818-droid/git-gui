//! Stage 视图:可折叠目录树。
//! j/k 移动 · l/h 展开折叠(目录) · Space 暂存/取消(文件或整目录)
//! · a 全暂存 · d 回滚(stash 兜底) · c 提交。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gitcore::{Error, FileState, FileStatus, Repo};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

/// Stage 视图的动作。
pub enum Action {
    None,
    Commit(String), // 提交,附带 SHA
    Back,           // 返回 Status
}

enum Mode {
    FileList,
    CommitInput,
    ConfirmDiscard,
}

/// 临时构造的目录树节点(每次重建可见行时生成,不长期持有)。
enum Node {
    Dir {
        name: String,
        path: PathBuf,
        expanded: bool,
        children: Vec<Node>,
    },
    File {
        name: String,
        path: PathBuf,
        state: FileState,
    },
}

/// 投影出的一条可见行(渲染 + 光标定位用)。
struct Row {
    depth: usize,
    name: String,
    path: PathBuf,
    is_dir: bool,
    expanded: bool,
    state: Option<FileState>,
}

pub struct StageView {
    files: Vec<FileStatus>,      // 扁平状态列表,真相源
    collapsed: HashSet<PathBuf>, // 已折叠的目录路径
    rows: Vec<Row>,              // 由 files + collapsed 派生的可见行
    cursor: usize,               // 光标(指向 rows)
    mode: Mode,
    commit_input: String,
    pending_discard: Option<Vec<PathBuf>>, // 待确认回滚的路径集
    message: String,
}

impl StageView {
    pub fn load(repo: &Repo) -> Result<Self, Error> {
        let st = repo.status()?;
        let mut view = StageView {
            files: st.files,
            collapsed: HashSet::new(),
            rows: Vec::new(),
            cursor: 0,
            mode: Mode::FileList,
            commit_input: String::new(),
            pending_discard: None,
            message: "Space 暂存/取消 · a 全暂存 · d 回滚 · l/h 展开/折叠 · c 提交 · q/Esc 返回"
                .into(),
        };
        view.rebuild_rows();
        Ok(view)
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match self.mode {
            Mode::FileList => self.handle_file_list(repo, c),
            Mode::CommitInput => self.handle_commit_input(repo, c),
            Mode::ConfirmDiscard => self.handle_confirm_discard(repo, c),
        }
    }

    fn handle_file_list(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' | '\x1b' => return Ok(Action::Back),
            'j' | crate::keys::DOWN if self.cursor + 1 < self.rows.len() => self.cursor += 1,
            'k' | crate::keys::UP if self.cursor > 0 => self.cursor -= 1,
            'l' | crate::keys::RIGHT => self.set_expand(true),
            'h' | crate::keys::LEFT => self.set_expand(false),
            '\n' | '\r' => self.toggle_expand(),
            ' ' => self.toggle_stage(repo)?,
            'a' => {
                repo.stage_all()?;
                self.reload(repo)?;
                self.message = "已暂存全部".into();
            }
            'd' => self.begin_discard(),
            'c' => {
                self.mode = Mode::CommitInput;
                self.commit_input.clear();
                self.message = "输入 commit 消息(Enter 提交 / Esc 取消)".into();
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_commit_input(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            '\x1b' => {
                // Esc
                self.mode = Mode::FileList;
                self.message = "已取消提交".into();
            }
            '\n' | '\r' => {
                // Enter
                if self.commit_input.trim().is_empty() {
                    self.message = "提交消息不能为空".into();
                    return Ok(Action::None);
                }
                let opts = gitcore::CommitOptions {
                    message: self.commit_input.clone(),
                    allow_empty: false,
                    amend: false,
                };
                match repo.commit(&opts) {
                    Ok(sha) => return Ok(Action::Commit(sha)),
                    Err(e) => {
                        self.message = format!("提交失败: {e}");
                        self.mode = Mode::FileList;
                    }
                }
            }
            '\x7f' | '\x08' => {
                // Backspace
                self.commit_input.pop();
            }
            c if !c.is_control() => {
                self.commit_input.push(c);
            }
            _ => {}
        }
        Ok(Action::None)
    }

    fn handle_confirm_discard(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'y' | 'Y' => {
                if let Some(paths) = self.pending_discard.take() {
                    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
                    repo.discard(&refs)?;
                    self.message = format!(
                        "已回滚 {} 项(改动已 stash 兜底,可在 Stash 视图 pop 找回)",
                        paths.len()
                    );
                    self.reload(repo)?;
                }
                self.mode = Mode::FileList;
            }
            _ => {
                self.pending_discard = None;
                self.mode = Mode::FileList;
                self.message = "已取消回滚".into();
            }
        }
        Ok(Action::None)
    }

    // ---- 折叠 / 展开 ----

    /// 把当前目录行设为展开(`want=true`)或折叠(`want=false`);非目录行忽略。
    fn set_expand(&mut self, want: bool) {
        let path = match self.rows.get(self.cursor) {
            Some(r) if r.is_dir => r.path.clone(),
            _ => return,
        };
        if want {
            self.collapsed.remove(&path);
        } else {
            self.collapsed.insert(path);
        }
        self.rebuild_rows();
    }

    fn toggle_expand(&mut self) {
        let path = match self.rows.get(self.cursor) {
            Some(r) if r.is_dir => r.path.clone(),
            _ => return,
        };
        if self.collapsed.contains(&path) {
            self.collapsed.remove(&path);
        } else {
            self.collapsed.insert(path);
        }
        self.rebuild_rows();
    }

    // ---- 暂存 / 取消 ----

    fn toggle_stage(&mut self, repo: &Repo) -> Result<(), Error> {
        let (is_dir, path, state) = match self.rows.get(self.cursor) {
            Some(r) => (r.is_dir, r.path.clone(), r.state),
            None => return Ok(()),
        };
        if is_dir {
            let under: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|f| f.path.starts_with(&path))
                .map(|f| f.path.clone())
                .collect();
            // 全部已暂存 → 取消整组;否则 → 暂存整组(含部分暂存的)。
            let all_staged = self
                .files
                .iter()
                .filter(|f| f.path.starts_with(&path))
                .all(|f| matches!(f.state, FileState::Staged));
            let refs: Vec<&Path> = under.iter().map(|p| p.as_path()).collect();
            if all_staged {
                repo.unstage(&refs)?;
                self.message = format!("已取消暂存 {}/ ({} 个文件)", path.display(), under.len());
            } else {
                repo.stage(&refs)?;
                self.message = format!("已暂存 {}/ ({} 个文件)", path.display(), under.len());
            }
        } else {
            match state.expect("文件行必有状态") {
                FileState::Staged | FileState::StagedAndModified => {
                    repo.unstage(&[&path])?;
                    self.message = format!("已取消暂存 {}", path.display());
                }
                FileState::Modified | FileState::Untracked => {
                    repo.stage(&[&path])?;
                    self.message = format!("已暂存 {}", path.display());
                }
            }
        }
        self.reload(repo)?;
        Ok(())
    }

    // ---- 回滚 ----

    /// 收集当前行(文件或整目录)的回滚目标,进入二次确认。
    fn begin_discard(&mut self) {
        let (paths, label) = match self.rows.get(self.cursor) {
            Some(r) if r.is_dir => {
                let dir = r.path.clone();
                let ps: Vec<PathBuf> = self
                    .files
                    .iter()
                    .filter(|f| f.path.starts_with(&dir))
                    .map(|f| f.path.clone())
                    .collect();
                let label = format!("目录 {}/ 下 {} 个文件", dir.display(), ps.len());
                (ps, label)
            }
            Some(r) => (vec![r.path.clone()], format!("文件 {}", r.path.display())),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        self.pending_discard = Some(paths);
        self.mode = Mode::ConfirmDiscard;
        self.message = format!("确认回滚 {label}?改动将 stash 兜底,可 pop 找回 [y/n]");
    }

    // ---- 重建可见行 ----

    fn reload(&mut self, repo: &Repo) -> Result<(), Error> {
        let st = repo.status()?;
        self.files = st.files;
        self.rebuild_rows();
        Ok(())
    }

    fn rebuild_rows(&mut self) {
        let tree = build_tree(&self.files, &self.collapsed);
        let mut rows = Vec::new();
        flatten(&tree, 0, &mut rows);
        self.rows = rows;
        if self.cursor >= self.rows.len() {
            self.cursor = self.rows.len().saturating_sub(1);
        }
    }

    // ---- 渲染 ----

    pub fn render(&self, f: &mut Frame) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        f.render_widget(
            Paragraph::new(" Stage 视图 · 目录树(Space 暂存/取消 · d 回滚)"),
            chunks[0],
        );

        match self.mode {
            Mode::CommitInput => self.render_commit_input(f, chunks[1]),
            _ => self.render_tree(f, chunks[1]),
        }

        f.render_widget(
            Paragraph::new(self.message.clone()).block(Block::bordered()),
            chunks[2],
        );
    }

    fn render_tree(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = Vec::new();
        if self.rows.is_empty() {
            lines.push(Line::from("(工作区干净,无文件改动)"));
        } else {
            for (i, row) in self.rows.iter().enumerate() {
                let indent = "  ".repeat(row.depth);
                let (prefix, color, label) = if row.is_dir {
                    let arrow = if row.expanded { "▾ " } else { "▸ " };
                    (arrow.to_string(), Color::Cyan, format!("{}/", row.name))
                } else {
                    let (p, c) = match row.state.expect("文件行必有状态") {
                        FileState::Staged => ("● ", Color::Green),
                        FileState::Modified => ("  ", Color::Red),
                        FileState::Untracked => ("? ", Color::Gray),
                        FileState::StagedAndModified => ("◐ ", Color::Yellow),
                    };
                    (p.to_string(), c, row.name.clone())
                };
                let mut style = Style::default().fg(color);
                if i == self.cursor {
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
                .block(Block::bordered().title(" 文件树 "))
                .scroll((
                    crate::scroll::follow(self.cursor, area.height.saturating_sub(2)),
                    0,
                )),
            area,
        );
    }

    fn render_commit_input(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let text = format!("消息: {}", self.commit_input);
        f.render_widget(
            Paragraph::new(text).block(Block::bordered().title(" Commit 消息 ")),
            area,
        );
    }
}

// ===== 目录树构造(纯函数,便于单测) =====

/// 把扁平的文件状态列表构造成目录树;`collapsed` 中的目录标记为已折叠。
fn build_tree(files: &[FileStatus], collapsed: &HashSet<PathBuf>) -> Vec<Node> {
    let mut roots: Vec<Node> = Vec::new();
    for f in files {
        let comps: Vec<String> = f
            .path
            .iter()
            .map(|c| c.to_string_lossy().into_owned())
            .collect();
        if comps.is_empty() {
            continue;
        }
        insert_path(&mut roots, &comps, PathBuf::new(), f.state, collapsed);
    }
    sort_nodes(&mut roots);
    roots
}

fn insert_path(
    level: &mut Vec<Node>,
    comps: &[String],
    prefix: PathBuf,
    state: FileState,
    collapsed: &HashSet<PathBuf>,
) {
    let head = &comps[0];
    let cur = prefix.join(head);
    if comps.len() == 1 {
        level.push(Node::File {
            name: head.clone(),
            path: cur,
            state,
        });
        return;
    }
    let idx = level
        .iter()
        .position(|n| matches!(n, Node::Dir { name, .. } if name == head));
    let idx = match idx {
        Some(i) => i,
        None => {
            let expanded = !collapsed.contains(&cur);
            level.push(Node::Dir {
                name: head.clone(),
                path: cur.clone(),
                expanded,
                children: Vec::new(),
            });
            level.len() - 1
        }
    };
    if let Node::Dir { children, .. } = &mut level[idx] {
        insert_path(children, &comps[1..], cur, state, collapsed);
    }
}

/// 目录在前、文件在后,各自按名字排序;递归到每层。
fn sort_nodes(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| {
        let is_file = |n: &Node| matches!(n, Node::File { .. });
        is_file(a)
            .cmp(&is_file(b))
            .then_with(|| name_of(a).cmp(name_of(b)))
    });
    for n in nodes.iter_mut() {
        if let Node::Dir { children, .. } = n {
            sort_nodes(children);
        }
    }
}

fn name_of(n: &Node) -> &str {
    match n {
        Node::Dir { name, .. } | Node::File { name, .. } => name,
    }
}

/// DFS 投影成可见行;折叠的目录不下钻其子节点。
fn flatten(nodes: &[Node], depth: usize, out: &mut Vec<Row>) {
    for n in nodes {
        match n {
            Node::Dir {
                name,
                path,
                expanded,
                children,
            } => {
                out.push(Row {
                    depth,
                    name: name.clone(),
                    path: path.clone(),
                    is_dir: true,
                    expanded: *expanded,
                    state: None,
                });
                if *expanded {
                    flatten(children, depth + 1, out);
                }
            }
            Node::File { name, path, state } => {
                out.push(Row {
                    depth,
                    name: name.clone(),
                    path: path.clone(),
                    is_dir: false,
                    expanded: false,
                    state: Some(*state),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs(path: &str, state: FileState) -> FileStatus {
        FileStatus {
            path: path.into(),
            state,
        }
    }

    fn rows_of(files: &[FileStatus], collapsed: &HashSet<PathBuf>) -> Vec<Row> {
        let tree = build_tree(files, collapsed);
        let mut rows = Vec::new();
        flatten(&tree, 0, &mut rows);
        rows
    }

    #[test]
    fn tree_groups_files_under_directory() {
        let files = vec![
            fs("src/a.rs", FileState::Modified),
            fs("src/b.rs", FileState::Staged),
            fs("README.md", FileState::Untracked),
        ];
        let rows = rows_of(&files, &HashSet::new());
        // 目录在前:src/(d0) → a.rs(d1) → b.rs(d1) → README.md(d0 文件)
        assert_eq!(rows.len(), 4);
        assert!(rows[0].is_dir && rows[0].name == "src" && rows[0].depth == 0);
        assert_eq!((rows[1].name.as_str(), rows[1].depth), ("a.rs", 1));
        assert_eq!((rows[2].name.as_str(), rows[2].depth), ("b.rs", 1));
        assert!(!rows[3].is_dir && rows[3].name == "README.md" && rows[3].depth == 0);
    }

    #[test]
    fn collapsed_dir_hides_children() {
        let files = vec![
            fs("src/a.rs", FileState::Modified),
            fs("src/b.rs", FileState::Staged),
        ];
        let mut collapsed = HashSet::new();
        collapsed.insert(PathBuf::from("src"));
        let rows = rows_of(&files, &collapsed);
        assert_eq!(rows.len(), 1, "折叠后只剩目录行");
        assert!(rows[0].is_dir && !rows[0].expanded);
    }

    #[test]
    fn nested_directories_nest_depth() {
        let files = vec![fs("a/b/c.rs", FileState::Modified)];
        let rows = rows_of(&files, &HashSet::new());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "a");
        assert_eq!(rows[1].name, "b");
        assert_eq!((rows[2].name.as_str(), rows[2].depth), ("c.rs", 2));
    }
}
