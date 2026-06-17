//! 冲突解决三栏视图:ours │ base │ theirs,逐块选择 + 魔法棒预填。
//! 支持多文件:顶部文件概览条 + n/p 自由切换,每个文件独立保留选择与进度。
//!
//! 解析/重建/写回/继续 都靠 gitcore 已测的 API,这里只管状态与渲染。

use gitcore::{Choice, Error, Repo, Segment, StashRef};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;
use std::path::PathBuf;

/// handle_key 给主循环的回执。
pub enum Action {
    None,
    Continue(Option<StashRef>),
    Abort(Option<StashRef>),
    Quit,
}

/// 单个冲突文件的解决状态(进入时预加载,各自独立保留)。
struct FileState {
    path: PathBuf,
    segments: Vec<Segment>, // refine 后的片段
    hunks: Vec<usize>,      // segments 中 Conflict 段下标
    choices: Vec<Choice>,   // 每个 hunk 的当前选择(与 hunks 对应)
    cursor: usize,          // 当前 hunk 在 hunks 里的位置
    written: bool,          // 已写回并 git add?
}

impl FileState {
    fn load(repo: &Repo, path: PathBuf) -> Result<Self, Error> {
        let segments = repo.read_conflict(&path)?;
        let hunks: Vec<usize> = segments
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, Segment::Conflict(_)).then_some(i))
            .collect();
        // 魔法棒预填:能自动的自动定,NeedsUser 暂默认 ours。
        let choices = hunks
            .iter()
            .map(|&i| match &segments[i] {
                Segment::Conflict(h) => h.magic().auto_choice().unwrap_or(Choice::Ours),
                _ => Choice::Ours,
            })
            .collect();
        Ok(FileState {
            path,
            segments,
            hunks,
            choices,
            cursor: 0,
            written: false,
        })
    }

    /// 还需处理?(尚未写回——含被魔法棒全解但还没落盘的文件)
    fn pending(&self) -> bool {
        !self.written
    }

    /// 当前 hunk 在 segments 里的下标。
    fn current_idx(&self) -> Option<usize> {
        self.hunks.get(self.cursor).copied()
    }

    /// 当前块前/后紧邻的自动定夺文本(同属一个原始冲突块的上下文)。
    fn auto_context(&self, hi: usize) -> (Vec<&str>, Vec<&str>) {
        let mut before = Vec::new();
        for seg in self.segments[..hi].iter().rev() {
            match seg {
                Segment::AutoResolved(t) => before.push(t.as_str()),
                _ => break,
            }
        }
        before.reverse();
        let mut after = Vec::new();
        for seg in &self.segments[hi + 1..] {
            match seg {
                Segment::AutoResolved(t) => after.push(t.as_str()),
                _ => break,
            }
        }
        (before, after)
    }

    /// 本文件被行级魔法棒自动解掉的行数。
    fn auto_lines(&self) -> usize {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::AutoResolved(t) => Some(t.lines().count()),
                _ => None,
            })
            .sum()
    }
}

/// 冲突解决视图状态:持有所有冲突文件,聚焦其中一个。
pub struct ConflictView {
    files: Vec<FileState>,
    idx: usize, // 当前文件
    autostash: Option<StashRef>,
    message: String,
}

impl ConflictView {
    pub fn load(
        repo: &Repo,
        files: Vec<PathBuf>,
        autostash: Option<StashRef>,
    ) -> Result<Self, Error> {
        let files = files
            .into_iter()
            .map(|p| FileState::load(repo, p))
            .collect::<Result<Vec<_>, _>>()?;
        // 初始聚焦第一个还有真冲突块的文件(全解文件可稍后 w 写回)。
        let idx = files.iter().position(|f| !f.hunks.is_empty()).unwrap_or(0);
        Ok(ConflictView {
            files,
            idx,
            autostash,
            message: "魔法棒已预填;o/t/b 改选 · j/k 切块 · n/p 切文件 · w 写回".into(),
        })
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' => return Ok(Action::Quit),
            'x' => return Ok(Action::Abort(self.autostash.take())),
            'c' => return Ok(Action::Continue(self.autostash.take())),
            'j' => {
                let f = &mut self.files[self.idx];
                if f.cursor + 1 < f.hunks.len() {
                    f.cursor += 1;
                }
            }
            'k' => {
                let f = &mut self.files[self.idx];
                f.cursor = f.cursor.saturating_sub(1);
            }
            'n' => {
                if self.idx + 1 < self.files.len() {
                    self.idx += 1;
                }
            }
            'p' => self.idx = self.idx.saturating_sub(1),
            'o' => self.set_choice(Choice::Ours),
            't' => self.set_choice(Choice::Theirs),
            'b' => self.set_choice(Choice::Base),
            'w' => self.write_and_advance(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    fn set_choice(&mut self, choice: Choice) {
        let f = &mut self.files[self.idx];
        if let Some(slot) = f.choices.get_mut(f.cursor) {
            *slot = choice;
        }
    }

    fn write_and_advance(&mut self, repo: &Repo) -> Result<(), Error> {
        let f = &self.files[self.idx];
        let text = gitcore::rebuild(&f.segments, &f.choices);
        repo.resolve_file(&f.path, &text)?;
        self.files[self.idx].written = true;

        match self.files.iter().position(FileState::pending) {
            Some(next) => {
                self.idx = next;
                self.message = format!("已写回;转到待处理文件 {}/{}", next + 1, self.files.len());
            }
            None => self.message = "全部文件已写回 —— 按 c 完成整合 / x 放弃".into(),
        }
        Ok(())
    }

    pub fn render(&self, f: &mut Frame) {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(f.area());

        let cur = &self.files[self.idx];
        let head = format!(
            " 冲突 {}/{}: {}   块 {}/{}   魔法棒自动解 {} 行",
            self.idx + 1,
            self.files.len(),
            cur.path.display(),
            (cur.cursor + 1).min(cur.hunks.len().max(1)),
            cur.hunks.len(),
            cur.auto_lines(),
        );
        f.render_widget(Paragraph::new(head), rows[0]);

        self.render_files(f, rows[1]);
        self.render_columns(f, rows[2]);

        let pick = match cur.choices.get(cur.cursor).copied() {
            Some(Choice::Ours) => "ours",
            Some(Choice::Theirs) => "theirs",
            Some(Choice::Base) => "base",
            None => "-",
        };
        let help = format!(
            "当前块选: {pick}   (绿色 ✓ = 魔法棒已自动解,无需处理)\no/t/b 选边 · j/k 切块 · n/p 切文件 · w 写回 · c 完成 · x 放弃 · q 退出\n{}",
            self.message
        );
        f.render_widget(Paragraph::new(help).block(Block::bordered()), rows[3]);
    }

    /// 文件概览条:✓ 已写回 / ○ 全自动待写 / ◆N N 块待解,当前文件反色。
    fn render_files(&self, f: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();
        for (i, fs) in self.files.iter().enumerate() {
            let name = fs
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| fs.path.display().to_string());
            let (label, color) = if fs.written {
                (format!(" ✓ {name} "), Color::Green)
            } else if fs.hunks.is_empty() {
                (format!(" ○ {name} "), Color::Green)
            } else {
                (format!(" ◆{} {name} ", fs.hunks.len()), Color::Yellow)
            };
            let mut style = Style::default().fg(color);
            if i == self.idx {
                style = style.add_modifier(Modifier::REVERSED);
            }
            spans.push(Span::styled(label, style));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_columns(&self, f: &mut Frame, area: Rect) {
        let cur = &self.files[self.idx];
        let cols = Layout::horizontal([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);
        let sel = cur.choices.get(cur.cursor).copied();

        let Some(hi) = cur.current_idx() else {
            for (i, title) in ["ours · 本地", "base · 祖先", "theirs · 远端"]
                .into_iter()
                .enumerate()
            {
                let p = Paragraph::new("✓ 已全部自动解决 — 按 w 写回")
                    .block(Block::bordered().title(format!("  {title}")));
                f.render_widget(p, cols[i]);
            }
            return;
        };
        let (before, after) = cur.auto_context(hi);
        let h = match &cur.segments[hi] {
            Segment::Conflict(h) => h,
            _ => return,
        };

        f.render_widget(
            column_panel(
                "ours · 本地",
                &before,
                &h.ours,
                &after,
                sel == Some(Choice::Ours),
            ),
            cols[0],
        );
        f.render_widget(
            column_panel(
                "base · 祖先",
                &before,
                &h.base,
                &after,
                sel == Some(Choice::Base),
            ),
            cols[1],
        );
        f.render_widget(
            column_panel(
                "theirs · 远端",
                &before,
                &h.theirs,
                &after,
                sel == Some(Choice::Theirs),
            ),
            cols[2],
        );
    }
}

/// 一栏:前置自动定夺上下文(绿 ✓) + 本栏冲突版本(黄) + 后置上下文(绿 ✓)。
fn column_panel<'a>(
    title: &str,
    before: &[&'a str],
    conflict: &'a str,
    after: &[&'a str],
    selected: bool,
) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    for &seg in before {
        lines.extend(seg.lines().map(auto_line));
    }
    for l in conflict.lines() {
        lines.push(conflict_line(l, selected));
    }
    for &seg in after {
        lines.extend(seg.lines().map(auto_line));
    }
    let block = if selected {
        Block::bordered()
            .title(format!("● {title}"))
            .border_style(Style::default().fg(Color::Green))
    } else {
        Block::bordered().title(format!("  {title}"))
    };
    Paragraph::new(lines).block(block)
}

/// 自动定夺行:绿色 + ✓ 前缀。
fn auto_line(l: &str) -> Line<'_> {
    Line::from(vec![
        Span::styled("✓ ", Style::default().fg(Color::Green)),
        Span::styled(l, Style::default().fg(Color::Green)),
    ])
}

/// 冲突行:黄色,选中栏加粗。
fn conflict_line(l: &str, selected: bool) -> Line<'_> {
    let mut style = Style::default().fg(Color::Yellow);
    if selected {
        style = style.add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(l, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcore::ConflictHunk;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn conflict_file(name: &str) -> FileState {
        FileState {
            path: name.into(),
            segments: vec![
                Segment::AutoResolved("A\n".into()),
                Segment::Conflict(ConflictHunk {
                    ours: "B\n".into(),
                    base: "3\n".into(),
                    theirs: "C\n".into(),
                }),
                Segment::AutoResolved("D\n".into()),
            ],
            hunks: vec![1],
            choices: vec![Choice::Theirs],
            cursor: 0,
            written: false,
        }
    }

    fn render_text(view: &ConflictView) -> (String, Vec<(String, Option<Color>)>) {
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        terminal.draw(|f| view.render(f)).unwrap();
        let buf = terminal.backend().buffer();
        let text = buf.content.iter().map(|c| c.symbol()).collect();
        let cells = buf
            .content
            .iter()
            .map(|c| (c.symbol().to_string(), c.style().fg))
            .collect();
        (text, cells)
    }

    // 离屏渲染:当前块前后紧邻的自动定夺行作为上下文(带 ✓)一并显示,
    // 冲突两边在场;上色锁死(✓/自动解=绿、冲突=黄)。
    #[test]
    fn renders_auto_resolved_context_around_conflict() {
        let view = ConflictView {
            files: vec![conflict_file("f.txt")],
            idx: 0,
            autostash: None,
            message: String::new(),
        };
        let (text, cells) = render_text(&view);

        assert!(text.contains('✓'), "应渲染自动定夺标记 ✓:\n{text}");
        assert!(
            text.contains('A') && text.contains('D'),
            "应带上冲突前后的自动定夺上下文"
        );
        assert!(
            text.contains('B') && text.contains('C'),
            "应显示冲突两边内容"
        );

        let fg_of = |want: &str| {
            cells
                .iter()
                .find(|(s, _)| s == want)
                .and_then(|(_, fg)| *fg)
        };
        assert_eq!(fg_of("✓"), Some(Color::Green), "✓ 标记应为绿色");
        assert_eq!(fg_of("B"), Some(Color::Yellow), "冲突行应为黄色");
    }

    // 多文件概览:全解文件标 ○(绿)、有冲突文件标 ◆N(黄),文件名都在场。
    #[test]
    fn overview_marks_resolved_and_pending_files() {
        let resolved = FileState {
            path: "auto.txt".into(),
            segments: vec![Segment::AutoResolved("x\n".into())],
            hunks: vec![],
            choices: vec![],
            cursor: 0,
            written: false,
        };
        let view = ConflictView {
            files: vec![resolved, conflict_file("manual.txt")],
            idx: 1,
            autostash: None,
            message: String::new(),
        };
        let (text, _) = render_text(&view);

        assert!(text.contains("auto.txt"), "概览应列出全解文件:\n{text}");
        assert!(text.contains("manual.txt"), "概览应列出待解文件:\n{text}");
        assert!(text.contains('○'), "全解文件应标 ○");
        assert!(text.contains('◆'), "待解文件应标 ◆");
    }

    fn temp_repo(tag: &str) -> (Repo, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tui-cf-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .unwrap();
        (Repo::open(&dir).unwrap(), dir)
    }

    // 状态机:n/p 切文件(边界不越界)、每个文件的选择独立保留。
    // (导航键不碰 repo,这里 repo 仅为 handle_key 签名占位。)
    #[test]
    fn navigation_switches_files_and_keeps_choices() {
        let (repo, dir) = temp_repo("nav");
        let mut view = ConflictView {
            files: vec![conflict_file("a.txt"), conflict_file("b.txt")],
            idx: 0,
            autostash: None,
            message: String::new(),
        };

        // a.txt 改选 base
        view.handle_key(&repo, 'b').unwrap();
        assert_eq!(view.files[0].choices[0], Choice::Base);

        // n 切到 b.txt,改选 ours
        view.handle_key(&repo, 'n').unwrap();
        assert_eq!(view.idx, 1);
        view.handle_key(&repo, 'o').unwrap();
        assert_eq!(view.files[1].choices[0], Choice::Ours);

        // n 到末尾不越界
        view.handle_key(&repo, 'n').unwrap();
        assert_eq!(view.idx, 1, "n 到末尾应停住");

        // p 切回 a.txt,选择仍是 base(独立保留)
        view.handle_key(&repo, 'p').unwrap();
        assert_eq!(view.idx, 0);
        assert_eq!(
            view.files[0].choices[0],
            Choice::Base,
            "切回文件后其选择应保留"
        );

        // p 到开头不越界
        view.handle_key(&repo, 'p').unwrap();
        assert_eq!(view.idx, 0, "p 到开头应停住");

        let _ = std::fs::remove_dir_all(dir);
    }
}
