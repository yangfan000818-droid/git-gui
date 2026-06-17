//! 冲突解决三栏视图:ours │ base │ theirs,逐块选择 + 魔法棒预填。
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

/// 冲突解决视图状态:一次聚焦一个文件、一个 hunk。
pub struct ConflictView {
    files: Vec<PathBuf>,
    idx: usize,             // 当前文件
    segments: Vec<Segment>, // 当前文件解析结果
    hunks: Vec<usize>,      // segments 中 Conflict 的下标
    choices: Vec<Choice>,   // 每个 hunk 的当前选择(与 hunks 对应)
    cursor: usize,          // 当前 hunk 在 hunks 里的位置
    autostash: Option<StashRef>,
    message: String,
}

impl ConflictView {
    pub fn load(
        repo: &Repo,
        files: Vec<PathBuf>,
        autostash: Option<StashRef>,
    ) -> Result<Self, Error> {
        let mut v = ConflictView {
            files,
            idx: 0,
            segments: Vec::new(),
            hunks: Vec::new(),
            choices: Vec::new(),
            cursor: 0,
            autostash,
            message: "魔法棒已预填可自动解的块;o/t/b 改选,w 写回".into(),
        };
        v.load_current(repo)?;
        Ok(v)
    }

    fn load_current(&mut self, repo: &Repo) -> Result<(), Error> {
        let segs = repo.read_conflict(&self.files[self.idx])?;
        let hunks: Vec<usize> = segs
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, Segment::Conflict(_)).then_some(i))
            .collect();
        // 魔法棒预填:能自动的自动定,NeedsUser 暂默认 ours。
        let choices = hunks
            .iter()
            .map(|&i| match &segs[i] {
                Segment::Conflict(h) => h.magic().auto_choice().unwrap_or(Choice::Ours),
                _ => Choice::Ours,
            })
            .collect();
        self.segments = segs;
        self.hunks = hunks;
        self.choices = choices;
        self.cursor = 0;
        Ok(())
    }

    pub fn handle_key(&mut self, repo: &Repo, c: char) -> Result<Action, Error> {
        match c {
            'q' => return Ok(Action::Quit),
            'x' => return Ok(Action::Abort(self.autostash.take())),
            'c' => return Ok(Action::Continue(self.autostash.take())),
            'j' => {
                if self.cursor + 1 < self.hunks.len() {
                    self.cursor += 1;
                }
            }
            'k' => self.cursor = self.cursor.saturating_sub(1),
            'o' => self.set_choice(Choice::Ours),
            't' => self.set_choice(Choice::Theirs),
            'b' => self.set_choice(Choice::Base),
            'w' => self.write_and_advance(repo)?,
            _ => {}
        }
        Ok(Action::None)
    }

    fn set_choice(&mut self, choice: Choice) {
        if let Some(slot) = self.choices.get_mut(self.cursor) {
            *slot = choice;
        }
    }

    fn write_and_advance(&mut self, repo: &Repo) -> Result<(), Error> {
        let text = gitcore::rebuild(&self.segments, &self.choices);
        repo.resolve_file(&self.files[self.idx], &text)?;
        if self.idx + 1 < self.files.len() {
            self.idx += 1;
            self.load_current(repo)?;
            self.message = format!("已写回,下一个文件 {}/{}", self.idx + 1, self.files.len());
        } else {
            self.message = "全部文件已写回 —— 按 c 完成整合 / x 放弃".into();
        }
        Ok(())
    }

    pub fn render(&self, f: &mut Frame) {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(f.area());

        let auto_lines: usize = self
            .segments
            .iter()
            .filter_map(|s| match s {
                Segment::AutoResolved(t) => Some(t.lines().count()),
                _ => None,
            })
            .sum();
        let head = format!(
            " 冲突 {}/{}: {}   块 {}/{}   魔法棒自动解 {auto_lines} 行",
            self.idx + 1,
            self.files.len(),
            self.files[self.idx].display(),
            (self.cursor + 1).min(self.hunks.len().max(1)),
            self.hunks.len()
        );
        f.render_widget(Paragraph::new(head), rows[0]);

        self.render_columns(f, rows[1]);

        let pick = match self.choices.get(self.cursor).copied() {
            Some(Choice::Ours) => "ours",
            Some(Choice::Theirs) => "theirs",
            Some(Choice::Base) => "base",
            None => "-",
        };
        let help = format!(
            "当前块选: {pick}   (绿色 ✓ = 魔法棒已自动解,无需处理)\no/t/b 选边 · j/k 切块 · w 写回本文件 · c 完成 · x 放弃 · q 退出\n{}",
            self.message
        );
        f.render_widget(Paragraph::new(help).block(Block::bordered()), rows[2]);
    }

    fn render_columns(&self, f: &mut Frame, area: Rect) {
        let cols = Layout::horizontal([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);
        let sel = self.choices.get(self.cursor).copied();

        let Some(hi) = self.current_idx() else {
            for (i, title) in ["ours · 本地", "base · 祖先", "theirs · 远端"]
                .into_iter()
                .enumerate()
            {
                let p = Paragraph::new("(无冲突块)")
                    .block(Block::bordered().title(format!("  {title}")));
                f.render_widget(p, cols[i]);
            }
            return;
        };
        let (before, after) = self.auto_context(hi);
        let h = match &self.segments[hi] {
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

    // 离屏渲染:当前块前后紧邻的自动定夺行应作为上下文(带 ✓)一并显示,
    // 冲突两边内容也在场。钉住 auto_context 收集 + 上色映射不回归。
    #[test]
    fn renders_auto_resolved_context_around_conflict() {
        let view = ConflictView {
            files: vec!["f.txt".into()],
            idx: 0,
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
            autostash: None,
            message: String::new(),
        };

        let mut terminal = Terminal::new(TestBackend::new(72, 14)).unwrap();
        terminal.draw(|f| view.render(f)).unwrap();
        let buf = terminal.backend().buffer();
        let text: String = buf.content.iter().map(|c| c.symbol()).collect();

        assert!(text.contains('✓'), "应渲染自动定夺标记 ✓:\n{text}");
        assert!(
            text.contains('A') && text.contains('D'),
            "应带上冲突前后的自动定夺上下文"
        );
        assert!(
            text.contains('B') && text.contains('C'),
            "应显示冲突两边内容"
        );

        // 上色锁死:✓ 与自动定夺行为绿、冲突行为黄。
        let fg_of = |want: &str| -> Option<Color> {
            buf.content
                .iter()
                .find(|c| c.symbol() == want)
                .and_then(|c| c.style().fg)
        };
        assert_eq!(fg_of("✓"), Some(Color::Green), "✓ 标记应为绿色");
        assert_eq!(fg_of("A"), Some(Color::Green), "自动定夺行应为绿色");
        assert_eq!(fg_of("B"), Some(Color::Yellow), "冲突行应为黄色");
    }
}
