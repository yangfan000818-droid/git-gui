//! 行级三路合并:在一个冲突块内部按行做 diff3,把"只有一边相对 base 改动"
//! 的行自动定夺,只把两边都改的行留给人。
//!
//! 背景:`git merge` 本身就是行级 diff3,因此凡 git 已圈成一个冲突块的区域,
//! 用"三方公共行作锚点"的标准 diff3 再跑一遍通常拆不动(同算法)。真正常见的
//! 可拆结构是:两边改动在 base 上重叠成一块,但块内首/尾若干行其实只有一边改。
//!
//! 算法:以 base 行为枢轴,用两个 LCS(ours-base / theirs-base)找三方公共行作
//! 锚点,把块切成区段;区段内若三段等长则逐行判定(等长 ⟺ 行级一一对应),
//! 不等长(结构性增删重叠)则整段保留冲突——宁可少自动,绝不误判两边都改的行。

/// 行级三路合并的一个结果块(相邻同类已合并)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Merge {
    /// 可自动定夺,文本已确定(单边改 / 两边改一致 / 未改)。
    Resolved(String),
    /// 两边都改且不一致,三段原样保留待人工。
    Conflict {
        ours: String,
        base: String,
        theirs: String,
    },
}

/// 对齐三路 diff 的一个区域类型(供 WebStorm 式三栏编辑器分类着色/操作)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum RegionKind {
    /// 三方一致,未改动。
    Unchanged,
    /// 仅本地(ours)相对 base 改动(theirs 同 base)。
    OursOnly,
    /// 仅对方(theirs)相对 base 改动(ours 同 base)。
    TheirsOnly,
    /// 两边改成一样。
    BothSame,
    /// 两边都改且不一致,需人工。
    Conflict,
}

/// 对齐三路 diff 的一个区域:三侧各自的原始行(含行尾换行,与 split_inclusive 一致)。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct MergeRegion {
    pub kind: RegionKind,
    pub ours: Vec<String>,
    pub base: Vec<String>,
    pub theirs: Vec<String>,
}

/// 对三段文本做行级 diff3,返回合并块序列。
pub(crate) fn merge3(ours: &str, base: &str, theirs: &str) -> Vec<Merge> {
    let o = lines(ours);
    let p = lines(base);
    let t = lines(theirs);

    // base 行 -> Some(对应行) 若被该侧保留(LCS 匹配);坐标随 base 单调递增。
    let a = lcs_map(&o, &p);
    let b = lcs_map(&t, &p);

    // 三方锚点:base 行同时被 ours、theirs 保留(三坐标随 base 单调递增)。
    let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
    for pj in 0..p.len() {
        if let (Some(oi), Some(ti)) = (a[pj], b[pj]) {
            anchors.push((oi, pj, ti));
        }
    }

    let mut acc = Acc::default();
    let (mut oi, mut pj, mut ti) = (0usize, 0usize, 0usize);
    let sentinel = (o.len(), p.len(), t.len());
    for (oa, pa, ta) in anchors.into_iter().chain(std::iter::once(sentinel)) {
        emit_segment(&o[oi..oa], &p[pj..pa], &t[ti..ta], &mut acc);
        if pa < p.len() {
            acc.push_resolved(p[pa]); // 锚点行三方相同
        }
        oi = oa + 1;
        pj = pa + 1;
        ti = ta + 1;
    }
    acc.finish()
}

/// 整文件对齐三路 diff:产出按区域分类的对齐结果(冲突 + 单边更改都保留三侧原行)。
/// 复用 merge3 的"base 为枢轴 + 三方锚点切段"骨架,只是按区域分类而非合并成文本。
pub(crate) fn merge_regions(ours: &str, base: &str, theirs: &str) -> Vec<MergeRegion> {
    let o = lines(ours);
    let p = lines(base);
    let t = lines(theirs);
    let a = lcs_map(&o, &p);
    let b = lcs_map(&t, &p);

    let mut anchors: Vec<(usize, usize, usize)> = Vec::new();
    for pj in 0..p.len() {
        if let (Some(oi), Some(ti)) = (a[pj], b[pj]) {
            anchors.push((oi, pj, ti));
        }
    }

    let mut acc = RegionAcc::default();
    let (mut oi, mut pj, mut ti) = (0usize, 0usize, 0usize);
    let sentinel = (o.len(), p.len(), t.len());
    for (oa, pa, ta) in anchors.into_iter().chain(std::iter::once(sentinel)) {
        emit_region_segment(&o[oi..oa], &p[pj..pa], &t[ti..ta], &mut acc);
        if pa < p.len() {
            acc.push(RegionKind::Unchanged, p[pa], p[pa], p[pa]); // 锚点行三方一致
        }
        oi = oa + 1;
        pj = pa + 1;
        ti = ta + 1;
    }
    acc.finish()
}

/// 区域版的段处理:等长段逐行判定,不等长段整段按"哪侧 == base"分类。
fn emit_region_segment(oc: &[&str], pc: &[&str], tc: &[&str], acc: &mut RegionAcc) {
    if oc.len() == pc.len() && tc.len() == pc.len() {
        for r in 0..pc.len() {
            let (o, p, t) = (oc[r], pc[r], tc[r]);
            let kind = if o == p && t == p {
                RegionKind::Unchanged
            } else if o == p {
                RegionKind::TheirsOnly
            } else if t == p {
                RegionKind::OursOnly
            } else if o == t {
                RegionKind::BothSame
            } else {
                RegionKind::Conflict
            };
            acc.push(kind, o, p, t);
        }
    } else if oc == tc {
        acc.push_seg(RegionKind::BothSame, oc, pc, tc);
    } else if oc == pc {
        acc.push_seg(RegionKind::TheirsOnly, oc, pc, tc);
    } else if tc == pc {
        acc.push_seg(RegionKind::OursOnly, oc, pc, tc);
    } else {
        acc.push_seg(RegionKind::Conflict, oc, pc, tc);
    }
}

/// 区域累积器:相邻同类区域合并,保留三侧原行。
#[derive(Default)]
struct RegionAcc {
    out: Vec<MergeRegion>,
    cur: Option<MergeRegion>,
}

impl RegionAcc {
    fn push(&mut self, kind: RegionKind, o: &str, p: &str, t: &str) {
        self.push_seg(kind, &[o], &[p], &[t]);
    }

    fn push_seg(&mut self, kind: RegionKind, oc: &[&str], pc: &[&str], tc: &[&str]) {
        if oc.is_empty() && pc.is_empty() && tc.is_empty() {
            return;
        }
        match &mut self.cur {
            Some(r) if r.kind == kind => {
                r.ours.extend(oc.iter().map(|s| s.to_string()));
                r.base.extend(pc.iter().map(|s| s.to_string()));
                r.theirs.extend(tc.iter().map(|s| s.to_string()));
            }
            _ => {
                if let Some(r) = self.cur.take() {
                    self.out.push(r);
                }
                self.cur = Some(MergeRegion {
                    kind,
                    ours: oc.iter().map(|s| s.to_string()).collect(),
                    base: pc.iter().map(|s| s.to_string()).collect(),
                    theirs: tc.iter().map(|s| s.to_string()).collect(),
                });
            }
        }
    }

    fn finish(mut self) -> Vec<MergeRegion> {
        if let Some(r) = self.cur.take() {
            self.out.push(r);
        }
        self.out
    }
}

/// 处理两个三方锚点之间的一段(三段各自的版本)。
fn emit_segment(oc: &[&str], pc: &[&str], tc: &[&str], acc: &mut Acc) {
    if oc.len() == pc.len() && tc.len() == pc.len() {
        // 三段行级一一对应,逐行判单边。
        for r in 0..pc.len() {
            let (o, p, t) = (oc[r], pc[r], tc[r]);
            if o == p {
                acc.push_resolved(t); // ours 未改(含两边都没改) → 取 theirs
            } else if t == p || o == t {
                acc.push_resolved(o); // theirs 未改,或两边改成一样 → 取 ours
            } else {
                acc.push_conflict(o, p, t); // 两边都改且不同
            }
        }
    } else if oc == tc {
        acc.push_resolved_seg(oc); // 两边改成一样(不等长但相同)
    } else if oc == pc {
        acc.push_resolved_seg(tc); // 仅 theirs 改(一侧等长不变、另一侧增删)
    } else if tc == pc {
        acc.push_resolved_seg(oc); // 仅 ours 改
    } else {
        acc.push_conflict_seg(oc, pc, tc); // 结构性双改,整段留人
    }
}

/// 把文本按行切分(保留行尾换行,与 parse_conflicts 一致)。
fn lines(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

/// 求 side 相对 base 的 LCS,返回长度 == base.len() 的映射:
/// base 第 j 行若在 LCS 中被 side 保留则 Some(side 行下标),否则 None。
fn lcs_map(side: &[&str], base: &[&str]) -> Vec<Option<usize>> {
    let n = side.len();
    let m = base.len();
    // dp[i][j] = LCS(side[i..], base[j..]) 的长度。
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if side[i] == base[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    // 回溯:沿 LCS 记录 base 行 -> side 行(i、j 同步单调推进)。
    let mut map = vec![None; m];
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if side[i] == base[j] {
            map[j] = Some(i);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    map
}

/// 输出累积器:相邻 Resolved 合并成一段、相邻 Conflict 合并成一段。
#[derive(Default)]
struct Acc {
    out: Vec<Merge>,
    resolved: String,
    co: String,
    cp: String,
    ct: String,
    in_conflict: bool,
}

impl Acc {
    fn push_resolved(&mut self, line: &str) {
        self.flush_conflict();
        self.resolved.push_str(line);
    }

    fn push_resolved_seg(&mut self, seg: &[&str]) {
        self.flush_conflict();
        for &l in seg {
            self.resolved.push_str(l);
        }
    }

    fn push_conflict(&mut self, o: &str, p: &str, t: &str) {
        self.flush_resolved();
        self.co.push_str(o);
        self.cp.push_str(p);
        self.ct.push_str(t);
        self.in_conflict = true;
    }

    fn push_conflict_seg(&mut self, oc: &[&str], pc: &[&str], tc: &[&str]) {
        self.flush_resolved();
        for &l in oc {
            self.co.push_str(l);
        }
        for &l in pc {
            self.cp.push_str(l);
        }
        for &l in tc {
            self.ct.push_str(l);
        }
        self.in_conflict = true;
    }

    fn flush_resolved(&mut self) {
        if !self.resolved.is_empty() {
            self.out
                .push(Merge::Resolved(std::mem::take(&mut self.resolved)));
        }
    }

    fn flush_conflict(&mut self) {
        if self.in_conflict {
            self.out.push(Merge::Conflict {
                ours: std::mem::take(&mut self.co),
                base: std::mem::take(&mut self.cp),
                theirs: std::mem::take(&mut self.ct),
            });
            self.in_conflict = false;
        }
    }

    fn finish(mut self) -> Vec<Merge> {
        self.flush_resolved();
        self.flush_conflict();
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(s: &str) -> Merge {
        Merge::Resolved(s.into())
    }
    fn cf(o: &str, b: &str, t: &str) -> Merge {
        Merge::Conflict {
            ours: o.into(),
            base: b.into(),
            theirs: t.into(),
        }
    }

    #[test]
    fn whole_block_only_ours_changed() {
        assert_eq!(
            merge3("a\nB\nc\n", "a\nb\nc\n", "a\nb\nc\n"),
            vec![res("a\nB\nc\n")]
        );
    }

    #[test]
    fn whole_block_only_theirs_changed() {
        assert_eq!(
            merge3("a\nb\nc\n", "a\nb\nc\n", "a\nb\nC\n"),
            vec![res("a\nb\nC\n")]
        );
    }

    #[test]
    fn disjoint_inner_changes_fully_resolve() {
        // ours 改首行、theirs 改末行,行级可全解、零冲突。
        assert_eq!(
            merge3("A\nb\nc\n", "a\nb\nc\n", "a\nb\nC\n"),
            vec![res("A\nb\nC\n")]
        );
    }

    #[test]
    fn same_line_both_changed_is_conflict() {
        assert_eq!(
            merge3("a\nX\nc\n", "a\nb\nc\n", "a\nY\nc\n"),
            vec![res("a\n"), cf("X\n", "b\n", "Y\n"), res("c\n")]
        );
    }

    #[test]
    fn overlap_splits_single_sides_out() {
        // 真实 git 块(E1):ours 改行1-2、theirs 改行2-3,重叠行2。
        // 行1 单边 ours、行2 真冲突、行3 单边 theirs。
        assert_eq!(
            merge3("A\nB\n4\n", "2\n3\n4\n", "2\nC\nD\n"),
            vec![res("A\n"), cf("B\n", "3\n", "C\n"), res("D\n")]
        );
    }

    #[test]
    fn overlap_strips_leading_single_side() {
        // 真实 git 块(E6 内段):仅首行单边 ours,其余两行两边都改。
        assert_eq!(
            merge3("A\nB\nC\n", "2\n3\n4\n", "2\nQ\nD\n"),
            vec![res("A\n"), cf("B\nC\n", "3\n4\n", "Q\nD\n")]
        );
    }

    #[test]
    fn both_insert_same_lines() {
        assert_eq!(
            merge3("a\nb\nc\n", "a\nc\n", "a\nb\nc\n"),
            vec![res("a\nb\nc\n")]
        );
    }

    #[test]
    fn both_insert_different_lines_conflict() {
        // base 段为空(纯插入位)。
        assert_eq!(
            merge3("a\nX\nc\n", "a\nc\n", "a\nY\nc\n"),
            vec![res("a\n"), cf("X\n", "", "Y\n"), res("c\n")]
        );
    }

    #[test]
    fn unequal_rewrite_stays_conflict() {
        // ours 把 1 行改成 2 行、theirs 改成 1 行:结构性双改,保守整段留人。
        assert_eq!(
            merge3("X\nY\n", "2\n", "Z\n"),
            vec![cf("X\nY\n", "2\n", "Z\n")]
        );
    }

    // ── merge_regions(对齐三路 diff) ──

    fn kinds(rs: &[MergeRegion]) -> Vec<RegionKind> {
        rs.iter().map(|r| r.kind).collect()
    }

    #[test]
    fn regions_classify_unchanged_ours_theirs_conflict() {
        // l1 三方同;l2 仅 ours 改;l3 仅 theirs 改;l4 两边都改且不同。
        let rs = merge_regions(
            "l1\nOURS2\nl3\nA4\n",
            "l1\nl2\nl3\nl4\n",
            "l1\nl2\nTHEIRS3\nB4\n",
        );
        assert_eq!(
            kinds(&rs),
            vec![
                RegionKind::Unchanged,
                RegionKind::OursOnly,
                RegionKind::TheirsOnly,
                RegionKind::Conflict,
            ]
        );
        // 区域保留三侧原行。
        assert_eq!(rs[1].ours, vec!["OURS2\n"]);
        assert_eq!(rs[1].theirs, vec!["l2\n"]);
        assert_eq!(rs[3].ours, vec!["A4\n"]);
        assert_eq!(rs[3].theirs, vec!["B4\n"]);
    }

    #[test]
    fn regions_both_same_change() {
        let rs = merge_regions("a\nX\nc\n", "a\nb\nc\n", "a\nX\nc\n");
        assert_eq!(
            kinds(&rs),
            vec![
                RegionKind::Unchanged,
                RegionKind::BothSame,
                RegionKind::Unchanged
            ]
        );
    }

    #[test]
    fn regions_consecutive_same_kind_merged() {
        // ours 连改两行:应合并成一个 OursOnly 区域(两行)。
        let rs = merge_regions("A\nB\nc\n", "a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(
            kinds(&rs),
            vec![RegionKind::OursOnly, RegionKind::Unchanged]
        );
        assert_eq!(rs[0].ours, vec!["A\n", "B\n"]);
    }
}
