//! 冲突解决三栏视图:ours │ base │ theirs,逐块选择 + 魔法棒预填。
//!
//! 解析/重建/写回/继续 都靠 gitcore 已测的 API,这里只管状态与渲染。

use gitcore::{Choice, ConflictHunk, Error, Repo, Segment, StashRef};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
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
                Segment::Clean(_) => Choice::Ours,
            })
            .collect();
        self.segments = segs;
        self.hunks = hunks;
        self.choices = choices;
        self.cursor = 0;
        Ok(())
    }

    fn current(&self) -> Option<&ConflictHunk> {
        self.hunks
            .get(self.cursor)
            .and_then(|&i| match &self.segments[i] {
                Segment::Conflict(h) => Some(h),
                Segment::Clean(_) => None,
            })
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

        let head = format!(
            " 冲突 {}/{}: {}   块 {}/{}",
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
            "当前块选: {pick}\no/t/b 选边 · j/k 切块 · w 写回本文件 · c 完成 · x 放弃 · q 退出\n{}",
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

        let (ours, base, theirs) = match self.current() {
            Some(h) => (h.ours.as_str(), h.base.as_str(), h.theirs.as_str()),
            None => ("(无冲突块)", "", ""),
        };
        let sel = self.choices.get(self.cursor).copied();

        f.render_widget(
            panel("ours · 本地", ours, sel == Some(Choice::Ours)),
            cols[0],
        );
        f.render_widget(
            panel("base · 祖先", base, sel == Some(Choice::Base)),
            cols[1],
        );
        f.render_widget(
            panel("theirs · 远端", theirs, sel == Some(Choice::Theirs)),
            cols[2],
        );
    }
}

fn panel<'a>(title: &str, body: &'a str, selected: bool) -> Paragraph<'a> {
    let mut block = Block::bordered();
    if selected {
        block = block
            .title(format!("● {title}"))
            .border_style(Style::default().fg(Color::Green));
    } else {
        block = block.title(format!("  {title}"));
    }
    Paragraph::new(body).block(block)
}
