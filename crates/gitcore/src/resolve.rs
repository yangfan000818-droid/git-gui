//! 冲突解决:把冲突文件解析成片段,魔法棒分类,按选择重建写回。
//!
//! 三版本来自 git 的 zdiff3 冲突标记(整合时强制开启),
//! 因此无需自己实现 diff3,直接解析标记即可。

use crate::{Error, Repo};
use std::path::Path;

/// 文件的一个片段:无冲突文本,或一个冲突块。
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// 冲突块外的原文。
    Clean(String),
    /// 行级魔法棒在冲突块内自动定夺的文本(下游同 Clean 直接输出)。
    AutoResolved(String),
    Conflict(ConflictHunk),
}

/// 一个冲突块的三个版本。
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictHunk {
    pub ours: String,
    pub base: String,
    pub theirs: String,
}

/// 魔法棒对一个 hunk 的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// ours 相对 base 没变 → 自动取 theirs。
    AutoTheirs,
    /// theirs 相对 base 没变 → 自动取 ours。
    AutoOurs,
    /// 两边都改了 → 需人工。
    NeedsUser,
}

/// 重建时对一个冲突块的选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Ours,
    Theirs,
    Base,
}

impl ConflictHunk {
    /// 魔法棒:依据三版本判断能否自动解决(整块级)。
    pub fn magic(&self) -> Resolution {
        if self.ours == self.base {
            Resolution::AutoTheirs
        } else if self.theirs == self.base {
            Resolution::AutoOurs
        } else {
            Resolution::NeedsUser
        }
    }

    /// 行级魔法棒:在 hunk 内部跑行级 diff3,把只有一边改动的行自动定夺,
    /// 返回细分后的片段序列——整块可解 → 单个 AutoResolved;部分可解 →
    /// AutoResolved/Conflict 交替;完全不可解 → 单个 Conflict。
    pub fn refine(&self) -> Vec<Segment> {
        crate::diff3::merge3(&self.ours, &self.base, &self.theirs)
            .into_iter()
            .map(|m| match m {
                crate::diff3::Merge::Resolved(t) => Segment::AutoResolved(t),
                crate::diff3::Merge::Conflict { ours, base, theirs } => {
                    Segment::Conflict(ConflictHunk { ours, base, theirs })
                }
            })
            .collect()
    }
}

impl Resolution {
    /// 魔法棒能自动定夺时给出选择,否则 None(需人工)。
    pub fn auto_choice(self) -> Option<Choice> {
        match self {
            Resolution::AutoOurs => Some(Choice::Ours),
            Resolution::AutoTheirs => Some(Choice::Theirs),
            Resolution::NeedsUser => None,
        }
    }
}

/// 解析 zdiff3 风格的冲突文件成片段序列;标记行本身被丢弃。
pub fn parse_conflicts(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut clean = String::new();
    let mut iter = text.split_inclusive('\n');

    while let Some(line) = iter.next() {
        if line.starts_with("<<<<<<<") {
            if !clean.is_empty() {
                segments.push(Segment::Clean(std::mem::take(&mut clean)));
            }
            let mut ours = String::new();
            let mut base = String::new();
            let mut theirs = String::new();
            for l in iter.by_ref() {
                if l.starts_with("|||||||") {
                    break;
                }
                ours.push_str(l);
            }
            for l in iter.by_ref() {
                if l.starts_with("=======") {
                    break;
                }
                base.push_str(l);
            }
            for l in iter.by_ref() {
                if l.starts_with(">>>>>>>") {
                    break;
                }
                theirs.push_str(l);
            }
            segments.push(Segment::Conflict(ConflictHunk { ours, base, theirs }));
        } else {
            clean.push_str(line);
        }
    }
    if !clean.is_empty() {
        segments.push(Segment::Clean(clean));
    }
    segments
}

/// 对片段序列做行级精化:每个冲突块内部跑行级 diff3,把只有一边改动的子区域
/// 自动定夺,展开成 Clean/Conflict 子序列;Clean 段原样保留。
pub fn refine_segments(segments: Vec<Segment>) -> Vec<Segment> {
    let mut out = Vec::with_capacity(segments.len());
    for seg in segments {
        match seg {
            Segment::Conflict(h) => out.extend(h.refine()),
            clean => out.push(clean),
        }
    }
    out
}

/// 按每个冲突块的选择重建文件文本;choices 与 Conflict 段一一对应,
/// 不足处默认取 ours。
pub fn rebuild(segments: &[Segment], choices: &[Choice]) -> String {
    let mut out = String::new();
    let mut ci = 0;
    for seg in segments {
        match seg {
            Segment::Clean(t) | Segment::AutoResolved(t) => out.push_str(t),
            Segment::Conflict(h) => {
                let choice = choices.get(ci).copied().unwrap_or(Choice::Ours);
                ci += 1;
                out.push_str(match choice {
                    Choice::Ours => &h.ours,
                    Choice::Theirs => &h.theirs,
                    Choice::Base => &h.base,
                });
            }
        }
    }
    out
}

/// 读取一个冲突文件并解析成片段。
pub(crate) fn read_conflict(repo: &Repo, path: &Path) -> Result<Vec<Segment>, Error> {
    let text = std::fs::read_to_string(repo.workdir().join(path))?;
    Ok(refine_segments(parse_conflicts(&text)))
}

/// 写回解决结果并 git add(标记冲突已解决)。
pub(crate) fn write_resolution(repo: &Repo, path: &Path, text: &str) -> Result<(), Error> {
    std::fs::write(repo.workdir().join(path), text)?;
    let p = path
        .to_str()
        .ok_or_else(|| Error::Parse("路径含非法字符".into()))?;
    repo.git(&["add", "--", p])?;
    Ok(())
}
