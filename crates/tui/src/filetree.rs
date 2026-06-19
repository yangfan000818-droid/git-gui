//! 通用文件路径树:把一组路径(改动文件)构造成可折叠目录树的可见行。
//! 供 Diff / Log 详情的左侧文件菜单栏复用(纯逻辑,可单测)。

use std::collections::HashSet;
use std::path::PathBuf;

pub enum RowKind {
    Dir { expanded: bool },
    File { index: usize }, // 原始路径列表中的下标
}

/// 投影出的一条可见树行。
pub struct TreeRow {
    pub depth: usize,
    pub name: String,
    pub path: PathBuf, // 完整相对路径(目录=目录路径,文件=文件路径)
    pub kind: RowKind,
}

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
        index: usize,
    },
}

/// 由路径列表 + 折叠集合构造可见树行(折叠目录不下钻);File 行带其原始下标。
pub fn build_rows(paths: &[String], collapsed: &HashSet<PathBuf>) -> Vec<TreeRow> {
    let mut roots: Vec<Node> = Vec::new();
    for (index, p) in paths.iter().enumerate() {
        let comps: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
        if comps.is_empty() {
            continue;
        }
        insert(&mut roots, &comps, PathBuf::new(), index, collapsed);
    }
    sort_nodes(&mut roots);
    let mut rows = Vec::new();
    flatten(&roots, 0, &mut rows);
    rows
}

fn insert(
    level: &mut Vec<Node>,
    comps: &[&str],
    prefix: PathBuf,
    index: usize,
    collapsed: &HashSet<PathBuf>,
) {
    let head = comps[0];
    let cur = prefix.join(head);
    if comps.len() == 1 {
        level.push(Node::File {
            name: head.to_string(),
            path: cur,
            index,
        });
        return;
    }
    let pos = level
        .iter()
        .position(|n| matches!(n, Node::Dir { name, .. } if name == head));
    let pos = match pos {
        Some(i) => i,
        None => {
            let expanded = !collapsed.contains(&cur);
            level.push(Node::Dir {
                name: head.to_string(),
                path: cur.clone(),
                expanded,
                children: Vec::new(),
            });
            level.len() - 1
        }
    };
    if let Node::Dir { children, .. } = &mut level[pos] {
        insert(children, &comps[1..], cur, index, collapsed);
    }
}

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

fn flatten(nodes: &[Node], depth: usize, out: &mut Vec<TreeRow>) {
    for n in nodes {
        match n {
            Node::Dir {
                name,
                path,
                expanded,
                children,
            } => {
                out.push(TreeRow {
                    depth,
                    name: name.clone(),
                    path: path.clone(),
                    kind: RowKind::Dir {
                        expanded: *expanded,
                    },
                });
                if *expanded {
                    flatten(children, depth + 1, out);
                }
            }
            Node::File { name, path, index } => {
                out.push(TreeRow {
                    depth,
                    name: name.clone(),
                    path: path.clone(),
                    kind: RowKind::File { index: *index },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_directory_with_indices() {
        let paths = vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "README.md".to_string(),
        ];
        let rows = build_rows(&paths, &HashSet::new());
        // 目录在前:src/(dir) → a.rs(file 0) → b.rs(file 1) → README.md(file 2)
        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0].kind, RowKind::Dir { .. }) && rows[0].name == "src");
        assert!(matches!(rows[1].kind, RowKind::File { index: 0 }) && rows[1].depth == 1);
        assert!(matches!(rows[2].kind, RowKind::File { index: 1 }));
        assert!(matches!(rows[3].kind, RowKind::File { index: 2 }) && rows[3].depth == 0);
    }

    #[test]
    fn collapsed_dir_hides_children() {
        let paths = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let mut collapsed = HashSet::new();
        collapsed.insert(PathBuf::from("src"));
        let rows = build_rows(&paths, &collapsed);
        assert_eq!(rows.len(), 1, "折叠后只剩目录行");
        assert!(matches!(rows[0].kind, RowKind::Dir { expanded: false }));
    }

    #[test]
    fn nested_dirs_increase_depth() {
        let paths = vec!["a/b/c.rs".to_string()];
        let rows = build_rows(&paths, &HashSet::new());
        assert_eq!(rows.len(), 3);
        assert_eq!((rows[2].name.as_str(), rows[2].depth), ("c.rs", 2));
        assert!(matches!(rows[2].kind, RowKind::File { index: 0 }));
    }
}
