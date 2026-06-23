//! gitcore 集成测试:在临时 git 仓库上验证真实行为。
//! 每个测试自建临时 repo、用完即删。

use gitcore::{
    CancelToken, IntegrationStrategy, PendingConflicts, RebaseAction, RebaseItem, Repo,
    SwitchOutcome, UpdateOptions, UpdateOutcome,
};
use std::path::{Path, PathBuf};
use std::process::Command;

fn unique_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "gitcore-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &PathBuf, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed in {dir:?}");
}

fn init_repo(tag: &str) -> PathBuf {
    let dir = unique_dir(tag);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    dir
}

fn bare_remote(tag: &str) -> PathBuf {
    let dir = unique_dir(tag);
    git(&dir, &["init", "--bare", "-q", "-b", "main"]);
    dir
}

fn clone(remote: &Path, tag: &str) -> PathBuf {
    let dir = unique_dir(tag);
    git(&dir, &["clone", "-q", remote.to_str().unwrap(), "."]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    dir
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn commit_all(dir: &PathBuf, msg: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-qm", msg]);
}

fn cleanup(dirs: &[&PathBuf]) {
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn clean_repo_reports_branch_and_not_dirty() {
    let dir = init_repo("clean");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let st = Repo::open(&dir).unwrap().status().unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert!(!st.dirty, "刚提交完应当干净");
    assert_eq!((st.ahead, st.behind), (0, 0));
    assert!(st.upstream.is_none(), "没设 remote 不应有 upstream");

    cleanup(&[&dir]);
}

#[test]
fn uncommitted_change_makes_repo_dirty() {
    let dir = init_repo("dirty");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");
    write(&dir, "a.txt", "changed");

    assert!(
        Repo::open(&dir).unwrap().status().unwrap().dirty,
        "改了未提交应当 dirty"
    );

    cleanup(&[&dir]);
}

#[test]
fn execute_update_fast_forwards_when_behind() {
    // a 先建仓推到裸库;b 克隆后,a 再推一个提交 → b 落后 1。
    let a = init_repo("ff-a");
    write(&a, "f.txt", "1");
    commit_all(&a, "c1");

    let remote = bare_remote("ff-remote");
    git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);

    let b = clone(&remote, "ff-b"); // 拿到 c1,track origin/main

    write(&a, "f.txt", "2");
    commit_all(&a, "c2");
    git(&a, &["push", "-q", "origin", "main"]);

    let outcome = Repo::open(&b)
        .unwrap()
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap();
    assert!(
        matches!(outcome, UpdateOutcome::FastForwarded { commits: 1 }),
        "落后 1 应快进 1,实际 {outcome:?}"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn parse_and_rebuild_roundtrip() {
    use gitcore::{parse_conflicts, rebuild, Choice};
    let text =
        "line1\n<<<<<<< HEAD\nY\n||||||| base\nline2\n=======\nX\n>>>>>>> origin/main\nline3\n";
    let segs = parse_conflicts(text);
    assert_eq!(rebuild(&segs, &[Choice::Theirs]), "line1\nX\nline3\n");
    assert_eq!(rebuild(&segs, &[Choice::Ours]), "line1\nY\nline3\n");
    assert_eq!(rebuild(&segs, &[Choice::Base]), "line1\nline2\nline3\n");
}

#[test]
fn magic_classifies_hunks() {
    use gitcore::{ConflictHunk, Resolution};
    let only_theirs = ConflictHunk {
        ours: "x\n".into(),
        base: "x\n".into(),
        theirs: "y\n".into(),
    };
    assert_eq!(only_theirs.magic(), Resolution::AutoTheirs);
    let only_ours = ConflictHunk {
        ours: "y\n".into(),
        base: "x\n".into(),
        theirs: "x\n".into(),
    };
    assert_eq!(only_ours.magic(), Resolution::AutoOurs);
    let both = ConflictHunk {
        ours: "a\n".into(),
        base: "x\n".into(),
        theirs: "b\n".into(),
    };
    assert_eq!(both.magic(), Resolution::NeedsUser);
}

#[test]
fn refine_whole_block_single_side_auto_resolved() {
    use gitcore::{ConflictHunk, Segment};
    // 整块只有 ours 改 → 行级魔法棒全自动,无残留冲突。
    let h = ConflictHunk {
        ours: "a\nB\nc\n".into(),
        base: "a\nb\nc\n".into(),
        theirs: "a\nb\nc\n".into(),
    };
    assert_eq!(h.refine(), vec![Segment::AutoResolved("a\nB\nc\n".into())]);
}

#[test]
fn refine_splits_inner_single_sides() {
    use gitcore::{ConflictHunk, Segment};
    // git 圈成一块,但行级看:行1 单边 ours、行2 真冲突、行3 单边 theirs。
    let h = ConflictHunk {
        ours: "A\nB\n4\n".into(),
        base: "2\n3\n4\n".into(),
        theirs: "2\nC\nD\n".into(),
    };
    assert_eq!(
        h.refine(),
        vec![
            Segment::AutoResolved("A\n".into()),
            Segment::Conflict(ConflictHunk {
                ours: "B\n".into(),
                base: "3\n".into(),
                theirs: "C\n".into(),
            }),
            Segment::AutoResolved("D\n".into()),
        ]
    );
}

#[test]
fn refine_keeps_real_conflict_for_user() {
    use gitcore::{ConflictHunk, Segment};
    // 同一行两边都改 → 仍需人工,只剩中间一行冲突。
    let h = ConflictHunk {
        ours: "a\nX\nc\n".into(),
        base: "a\nb\nc\n".into(),
        theirs: "a\nY\nc\n".into(),
    };
    assert_eq!(
        h.refine(),
        vec![
            Segment::AutoResolved("a\n".into()),
            Segment::Conflict(ConflictHunk {
                ours: "X\n".into(),
                base: "b\n".into(),
                theirs: "Y\n".into(),
            }),
            Segment::AutoResolved("c\n".into()),
        ]
    );
}

#[test]
fn read_conflict_applies_line_level_magic() {
    use gitcore::{Choice, Segment};
    let dir = init_repo("linemagic");
    write(&dir, "x.txt", "init\n");
    commit_all(&dir, "i");
    // 手写一个 git 真实会产生的 zdiff3 冲突块:行1 单边 ours、行3 单边 theirs。
    let conflicted =
        "<<<<<<< ours\nA\nB\n4\n||||||| base\n2\n3\n4\n=======\n2\nC\nD\n>>>>>>> theirs\n";
    write(&dir, "x.txt", conflicted);

    let repo = Repo::open(&dir).unwrap();
    let segs = repo.read_conflict(&PathBuf::from("x.txt")).unwrap();

    // 行级魔法棒应自动解掉首尾单边行,只把中间一行留作冲突。
    let conflicts: Vec<_> = segs
        .iter()
        .filter_map(|s| match s {
            Segment::Conflict(h) => Some(h),
            _ => None,
        })
        .collect();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].base, "3\n");

    // 无论冲突行选哪边,单边行(A、D)都固定保留。
    assert_eq!(gitcore::rebuild(&segs, &[Choice::Theirs]), "A\nC\nD\n");
    assert_eq!(gitcore::rebuild(&segs, &[Choice::Ours]), "A\nB\nD\n");

    cleanup(&[&dir]);
}

// 造一个两边改同一行的真冲突;返回 (a, remote, b),b 处于冲突待解决。
fn setup_conflict(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let a = init_repo(&format!("{tag}-a"));
    write(&a, "f.txt", "1\n2\n3\n");
    commit_all(&a, "base");
    let remote = bare_remote(&format!("{tag}-remote"));
    git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);

    let b = clone(&remote, &format!("{tag}-b"));

    write(&a, "f.txt", "1\nX\n3\n");
    commit_all(&a, "a-change");
    git(&a, &["push", "-q", "origin", "main"]);

    write(&b, "f.txt", "1\nY\n3\n");
    commit_all(&b, "b-change");
    (a, remote, b)
}

#[test]
fn merge_conflict_resolve_and_continue() {
    let (a, remote, b) = setup_conflict("cf");
    let repo = Repo::open(&b).unwrap();

    let (files, autostash) = match repo
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap()
    {
        UpdateOutcome::Conflicted { files, autostash } => (files, autostash),
        other => panic!("应当冲突,实际 {other:?}"),
    };
    assert_eq!(files, vec![PathBuf::from("f.txt")]);

    let segs = repo.read_conflict(&files[0]).unwrap();
    let hunks: Vec<_> = segs
        .iter()
        .filter_map(|s| match s {
            gitcore::Segment::Conflict(h) => Some(h),
            _ => None,
        })
        .collect();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].magic(), gitcore::Resolution::NeedsUser);

    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo.resolve_file(&files[0], &resolved).unwrap();

    assert!(matches!(
        repo.continue_update(autostash, true).unwrap(),
        UpdateOutcome::Resolved
    ));
    assert_eq!(
        std::fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\nX\n3\n"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn merge_conflict_can_be_aborted() {
    let (a, remote, b) = setup_conflict("ab");
    let repo = Repo::open(&b).unwrap();

    let autostash = match repo
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap()
    {
        UpdateOutcome::Conflicted { autostash, .. } => autostash,
        other => panic!("应当冲突,实际 {other:?}"),
    };

    repo.abort_update(autostash).unwrap();
    let st = repo.status().unwrap();
    assert!(!st.dirty, "abort 后应回到干净");
    assert!(st.conflicted.is_empty());
    assert_eq!(
        std::fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\nY\n3\n"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn resume_finds_pending_conflict_and_restores_autostash() {
    let (a, remote, b) = setup_conflict("rs");
    // 让 b 工作区脏(无关文件 g.txt),迫使 execute_update 先 autostash。
    write(&b, "g.txt", "dirty\n");

    let repo = Repo::open(&b).unwrap();
    let out = repo
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap();
    assert!(
        matches!(
            out,
            UpdateOutcome::Conflicted {
                autostash: Some(_),
                ..
            }
        ),
        "应冲突且有 autostash,实际 {out:?}"
    );

    // 模拟中断:丢弃上面返回的 autostash,重新打开仓库后扫描恢复。
    let repo2 = Repo::open(&b).unwrap();
    let PendingConflicts { files, autostash } = repo2
        .resume_conflicts()
        .unwrap()
        .expect("应检测到未完成的整合");
    assert_eq!(files, vec![PathBuf::from("f.txt")]);
    assert!(autostash.is_some(), "应扫回 autostash");

    // 解决 + 完成,确认 autostash 还原(g.txt 回来)。
    let segs = repo2.read_conflict(&files[0]).unwrap();
    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo2.resolve_file(&files[0], &resolved).unwrap();
    assert!(matches!(
        repo2.continue_update(autostash, true).unwrap(),
        UpdateOutcome::Resolved
    ));
    assert_eq!(
        std::fs::read_to_string(b.join("g.txt")).unwrap(),
        "dirty\n",
        "autostash 应还原无关脏文件"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn rerere_replays_previous_resolution() {
    let (a, remote, b) = setup_conflict("rr");
    let repo = Repo::open(&b).unwrap();

    // 第一次:冲突 → 选 theirs 解决 → continue。rerere 记录解法。
    let (files, autostash) = match repo
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap()
    {
        UpdateOutcome::Conflicted { files, autostash } => (files, autostash),
        o => panic!("应冲突,实际 {o:?}"),
    };
    let segs = repo.read_conflict(&files[0]).unwrap();
    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo.resolve_file(&files[0], &resolved).unwrap();
    repo.continue_update(autostash, true).unwrap();

    // 撤销这次 merge,回到冲突前。
    git(&b, &["reset", "--hard", "HEAD~1"]);

    // 第二次:同样冲突 → rerere 重放 + 自动确认 → 无需人工,直接完成。
    match repo
        .execute_update(&UpdateOptions::default(), &CancelToken::default())
        .unwrap()
    {
        UpdateOutcome::Resolved => {}
        o => panic!("rerere 应全自动解决,实际 {o:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\nX\n3\n",
        "rerere 应重放上次 theirs 解法"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn rebase_conflict_resolve_and_continue() {
    let (a, remote, b) = setup_conflict("rb");
    let repo = Repo::open(&b).unwrap();
    let opts = UpdateOptions {
        strategy: IntegrationStrategy::Rebase,
        ignore_whitespace: true,
        ..Default::default()
    };

    let (files, autostash) = match repo.execute_update(&opts, &CancelToken::default()).unwrap() {
        UpdateOutcome::Conflicted { files, autostash } => (files, autostash),
        o => panic!("rebase 应产生冲突,实际 {o:?}"),
    };
    assert_eq!(files, vec![PathBuf::from("f.txt")]);

    // rebase 期间 HEAD 是被 replay 到的基(origin/main),
    // 所以 ours=远端(X)、theirs=本地提交(Y)——与 merge 相反。选 theirs 保留本地。
    let segs = repo.read_conflict(&files[0]).unwrap();
    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo.resolve_file(&files[0], &resolved).unwrap();

    assert!(matches!(
        repo.continue_update(autostash, true).unwrap(),
        UpdateOutcome::Resolved
    ));
    assert_eq!(
        std::fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\nY\n3\n",
        "rebase 选 theirs 应保留本地 Y"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn rebase_conflict_can_be_aborted() {
    let (a, remote, b) = setup_conflict("rba");
    let repo = Repo::open(&b).unwrap();
    let opts = UpdateOptions {
        strategy: IntegrationStrategy::Rebase,
        ignore_whitespace: true,
        ..Default::default()
    };

    let autostash = match repo.execute_update(&opts, &CancelToken::default()).unwrap() {
        UpdateOutcome::Conflicted { autostash, .. } => autostash,
        o => panic!("rebase 应产生冲突,实际 {o:?}"),
    };

    repo.abort_update(autostash).unwrap();
    let st = repo.status().unwrap();
    assert!(!st.dirty, "rebase abort 后应回到干净");
    assert!(st.conflicted.is_empty());
    assert_eq!(
        std::fs::read_to_string(b.join("f.txt")).unwrap(),
        "1\nY\n3\n",
        "rebase abort 应保留本地 b-change"
    );

    cleanup(&[&a, &remote, &b]);
}

// 造一个能自动合并的分叉(a、b 改不同文件,无冲突);返回 (a, remote, b),b ahead=1 behind=1。
fn setup_divergent_no_conflict(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let a = init_repo(&format!("{tag}-a"));
    write(&a, "f.txt", "base\n");
    commit_all(&a, "base");
    let remote = bare_remote(&format!("{tag}-remote"));
    git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);

    let b = clone(&remote, &format!("{tag}-b"));

    write(&a, "a.txt", "from-a\n");
    commit_all(&a, "a-change");
    git(&a, &["push", "-q", "origin", "main"]);

    write(&b, "b.txt", "from-b\n");
    commit_all(&b, "b-change");
    (a, remote, b)
}

// 整合阶段意外失败(非冲突)时,autostash 必须被还原,不能把脏改动遗弃在 stash 里。
#[cfg(unix)]
#[test]
fn update_restores_autostash_when_integration_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (a, remote, b) = setup_divergent_no_conflict("intfail");

    // 装一个必定失败的 pre-merge-commit hook:自动合并能完成,但创建 merge commit 前被拒。
    write(&b, ".git/hooks/pre-merge-commit", "#!/bin/sh\nexit 1\n");
    let hook = b.join(".git/hooks/pre-merge-commit");
    let mut perm = std::fs::metadata(&hook).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&hook, perm).unwrap();

    // 工作区弄脏(无关 untracked 文件)→ 触发 autostash。
    write(&b, "dirty.txt", "WIP\n");

    let repo = Repo::open(&b).unwrap();
    let result = repo.execute_update(&UpdateOptions::default(), &CancelToken::default());

    assert!(
        result.is_err(),
        "整合被 hook 拒绝应返回错误,实际 {result:?}"
    );

    // 关键:脏改动不能被遗弃在 stash 里 —— autostash 应已还原。
    let stashes = repo.stashes().unwrap();
    assert!(
        stashes.is_empty(),
        "整合失败后不应残留 autostash,实际 {stashes:?}"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("dirty.txt")).unwrap(),
        "WIP\n",
        "脏改动应被还原回工作区"
    );

    // 不应残留半完成的 merge。
    assert!(
        repo.status().unwrap().conflicted.is_empty(),
        "不应残留冲突/半完成整合"
    );

    cleanup(&[&a, &remote, &b]);
}

// ========== stage/commit/push 测试 ==========

#[test]
fn stage_and_commit_advances_head() {
    let dir = init_repo("sc");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();

    // 记录旧 HEAD
    let old_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let old_head = String::from_utf8_lossy(&old_head.stdout).trim().to_string();

    write(&dir, "b.txt", "world");
    repo.stage(&[Path::new("b.txt")]).unwrap();

    let opts = gitcore::CommitOptions {
        message: "add b".into(),
        allow_empty: false,
        amend: false,
        no_verify: false,
    };
    let sha = repo.commit(&opts).unwrap();
    assert_eq!(sha.len(), 8, "SHA 应为 8 位");

    // 验证 HEAD 改变
    let new_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let new_head = String::from_utf8_lossy(&new_head.stdout).trim().to_string();
    assert_ne!(old_head, new_head, "commit 应改变 HEAD");

    let st = repo.status().unwrap();
    assert!(!st.dirty, "commit 后应干净");
    assert_eq!(st.files.len(), 0);

    cleanup(&[&dir]);
}

#[test]
fn unstage_removes_from_index() {
    let dir = init_repo("us");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();
    write(&dir, "b.txt", "world");
    repo.stage(&[Path::new("b.txt")]).unwrap();

    let st = repo.status().unwrap();
    assert_eq!(st.files.len(), 1);
    assert!(matches!(st.files[0].state, gitcore::FileState::Staged));

    repo.unstage(&[Path::new("b.txt")]).unwrap();

    let st = repo.status().unwrap();
    assert_eq!(st.files.len(), 1);
    assert!(matches!(st.files[0].state, gitcore::FileState::Untracked));

    cleanup(&[&dir]);
}

// 回归:整个未跟踪目录,git 默认折叠成 "dir/" 单条目(尾斜杠),前端取 basename 得空名、
// 且目录无法 diff/单独暂存;status 必须用 -uall 把未跟踪目录展开到文件级。
#[test]
fn status_expands_untracked_directory_to_files() {
    let dir = init_repo("sx");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();
    std::fs::create_dir_all(dir.join("新目录")).unwrap();
    write(&dir, "新目录/x.txt", "x");
    write(&dir, "新目录/y.txt", "y");

    let paths: Vec<String> = repo
        .status()
        .unwrap()
        .files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        paths.iter().all(|p| !p.ends_with('/')),
        "未跟踪目录应展开到文件级,不应出现尾斜杠条目: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "新目录/x.txt") && paths.iter().any(|p| p == "新目录/y.txt"),
        "目录内文件应各自作为未跟踪条目出现: {paths:?}"
    );

    cleanup(&[&dir]);
}

#[test]
fn discard_reverts_to_head_with_stash_backup() {
    let dir = init_repo("dc");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();
    // 改一个已跟踪文件 + 加一个未跟踪文件
    write(&dir, "a.txt", "modified");
    write(&dir, "new.txt", "untracked");
    assert_eq!(repo.status().unwrap().files.len(), 2);

    repo.discard(&[Path::new("a.txt"), Path::new("new.txt")])
        .unwrap();

    // 工作区回到 HEAD:a.txt 恢复原内容,new.txt 被移除
    assert!(!repo.status().unwrap().dirty, "discard 后工作区应干净");
    assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "hello");
    assert!(!dir.join("new.txt").exists(), "未跟踪文件应被移除");

    // 兜底:改动进了 stash,pop 能完整找回
    assert_eq!(repo.stashes().unwrap().len(), 1, "应有一条兜底 stash");
    repo.stash_pop("stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "modified"
    );
    assert!(dir.join("new.txt").exists(), "pop 后未跟踪文件应找回");

    cleanup(&[&dir]);
}

#[test]
fn stage_hunk_stages_only_selected_change() {
    let dir = init_repo("sh");
    write(&dir, "f.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    commit_all(&dir, "init");
    // 改首尾两行 → 中间有足够间隔,应解析成两个分离 hunk
    write(&dir, "f.txt", "1-mod\n2\n3\n4\n5\n6\n7\n8\n9\n10-mod\n");

    let repo = Repo::open(&dir).unwrap();
    let files = repo.unstaged_diff().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].hunks.len(), 2, "首尾两处改动应是 2 个 hunk");

    // 只暂存第一个 hunk
    repo.stage_hunk(&files[0], &files[0].hunks[0]).unwrap();

    let staged = repo.staged_diff().unwrap();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].hunks.len(), 1, "暂存区只含第一处改动");
    let unstaged = repo.unstaged_diff().unwrap();
    assert_eq!(unstaged[0].hunks.len(), 1, "还剩第二处改动未暂存");

    cleanup(&[&dir]);
}

#[test]
fn unstage_hunk_removes_from_index() {
    let dir = init_repo("uh");
    write(&dir, "f.txt", "1\n2\n3\n");
    commit_all(&dir, "init");
    write(&dir, "f.txt", "1-mod\n2\n3\n");

    let repo = Repo::open(&dir).unwrap();
    repo.stage(&[Path::new("f.txt")]).unwrap();
    assert_eq!(repo.staged_diff().unwrap()[0].hunks.len(), 1);

    // 取消暂存这个 hunk
    let staged = repo.staged_diff().unwrap();
    repo.unstage_hunk(&staged[0], &staged[0].hunks[0]).unwrap();

    assert!(repo.staged_diff().unwrap().is_empty(), "取消后暂存区应空");
    assert_eq!(
        repo.unstaged_diff().unwrap()[0].hunks.len(),
        1,
        "改动回到未暂存"
    );

    cleanup(&[&dir]);
}

#[test]
fn stage_lines_stages_only_selected_lines() {
    let dir = init_repo("sl");
    write(&dir, "f.txt", "a\n");
    commit_all(&dir, "init");
    // 同一个 hunk 内新增 b、c 两行
    write(&dir, "f.txt", "a\nb\nc\n");

    let repo = Repo::open(&dir).unwrap();
    let files = repo.unstaged_diff().unwrap();
    let hunk = &files[0].hunks[0];
    let added: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| matches!(l.kind, gitcore::LineKind::Added))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(added.len(), 2, "新增 b、c 两行");

    // 只暂存第一行新增(b)
    repo.stage_lines(&files[0], hunk, &[added[0]]).unwrap();

    let staged_added: Vec<String> = repo.staged_diff().unwrap()[0].hunks[0]
        .lines
        .iter()
        .filter(|l| matches!(l.kind, gitcore::LineKind::Added))
        .map(|l| l.content.clone())
        .collect();
    assert_eq!(staged_added, vec!["b"], "暂存区只含选中的 b");

    let unstaged_added: Vec<String> = repo.unstaged_diff().unwrap()[0].hunks[0]
        .lines
        .iter()
        .filter(|l| matches!(l.kind, gitcore::LineKind::Added))
        .map(|l| l.content.clone())
        .collect();
    assert_eq!(unstaged_added, vec!["c"], "工作区还剩未选的 c");

    cleanup(&[&dir]);
}

#[test]
fn unstaged_diff_includes_untracked_files() {
    let dir = init_repo("ut");
    write(&dir, "a.txt", "x\n");
    commit_all(&dir, "init");
    // 改已跟踪 + 新增未跟踪
    write(&dir, "a.txt", "x\ny\n");
    write(&dir, "new.txt", "hello\nworld\n");

    let repo = Repo::open(&dir).unwrap();
    let files = repo.unstaged_diff().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"a.txt"), "含已跟踪改动");
    assert!(paths.contains(&"new.txt"), "含未跟踪文件");

    // new.txt 内容全部为新增行
    let nf = files.iter().find(|f| f.path == "new.txt").unwrap();
    let added: Vec<&str> = nf.hunks[0]
        .lines
        .iter()
        .filter(|l| matches!(l.kind, gitcore::LineKind::Added))
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(added, vec!["hello", "world"]);

    // 暂存未跟踪文件的 hunk → 进入暂存区(等同 git add)
    repo.stage_hunk(nf, &nf.hunks[0]).unwrap();
    assert!(
        repo.staged_diff()
            .unwrap()
            .iter()
            .any(|f| f.path == "new.txt"),
        "stage_hunk 后 new.txt 应已暂存"
    );

    cleanup(&[&dir]);
}

#[test]
fn unstaged_diff_excludes_ignored_files() {
    let dir = init_repo("ig");
    write(&dir, "a.txt", "x\n");
    write(&dir, ".gitignore", "ignored.txt\n");
    commit_all(&dir, "init");
    write(&dir, "ignored.txt", "secret\n"); // 被 .gitignore 忽略
    write(&dir, "visible.txt", "new\n"); // 正常未跟踪

    let repo = Repo::open(&dir).unwrap();
    let paths: Vec<String> = repo
        .unstaged_diff()
        .unwrap()
        .iter()
        .map(|f| f.path.clone())
        .collect();
    assert!(
        paths.iter().any(|p| p == "visible.txt"),
        "正常未跟踪文件应显示"
    );
    assert!(
        !paths.iter().any(|p| p == "ignored.txt"),
        "gitignore 忽略的文件不应显示"
    );

    cleanup(&[&dir]);
}

#[test]
fn commit_files_parses_changed_files() {
    let dir = init_repo("cf");
    write(&dir, "a.txt", "1\n");
    write(&dir, "b.txt", "2\n");
    commit_all(&dir, "two files");

    let repo = Repo::open(&dir).unwrap();
    let sha = repo.log(&gitcore::LogOptions::default()).unwrap()[0]
        .sha
        .clone();
    let files = repo.commit_files(&sha).unwrap();

    assert_eq!(files.len(), 2, "该 commit 改了两个文件");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"a.txt"));
    assert!(paths.contains(&"b.txt"));

    cleanup(&[&dir]);
}

#[test]
fn log_graph_marks_commit_rows_and_merges() {
    let dir = init_repo("lg");
    write(&dir, "a.txt", "1");
    commit_all(&dir, "c1");
    write(&dir, "a.txt", "2");
    commit_all(&dir, "c2");
    // 造一个分支并 --no-ff 合并,产生拓扑图连接行
    git(&dir, &["checkout", "-q", "-b", "feat"]);
    write(&dir, "b.txt", "x");
    commit_all(&dir, "feat1");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "c.txt", "y");
    commit_all(&dir, "main1");
    git(&dir, &["merge", "--no-ff", "feat", "-m", "merge feat"]);

    let repo = Repo::open(&dir).unwrap();
    let rows = repo.log_graph(&gitcore::LogOptions::default()).unwrap();

    // 5 个提交:c1 c2 feat1 main1 merge
    let commit_rows: Vec<_> = rows.iter().filter(|r| r.entry.is_some()).collect();
    assert_eq!(commit_rows.len(), 5, "应有 5 个 commit 行");
    // full_sha 为完整 40 位,短 sha 是其前缀(供复制哈希用)
    let e = commit_rows[0].entry.as_ref().unwrap();
    assert_eq!(e.full_sha.len(), 40, "full_sha 应为 40 位");
    assert!(e.full_sha.starts_with(&e.sha), "短 sha 应是 full_sha 前缀");
    // 合并历史应产生至少一行纯连接行(无 commit)
    assert!(rows.iter().any(|r| r.entry.is_none()), "应有图形连接行");
    // commit 行带图形标记
    assert!(commit_rows[0].graph.contains('*'), "commit 行图形含 *");

    cleanup(&[&dir]);
}

#[test]
fn commit_empty_staging_fails() {
    let dir = init_repo("ce");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();
    let opts = gitcore::CommitOptions {
        message: "empty".into(),
        allow_empty: false,
        amend: false,
        no_verify: false,
    };

    let err = repo.commit(&opts).unwrap_err();
    assert!(
        matches!(err, gitcore::Error::Precondition(_)),
        "空暂存区应拒绝 commit"
    );

    cleanup(&[&dir]);
}

#[test]
fn push_to_bare_remote() {
    let remote = bare_remote("pbr");
    let dir = clone(&remote, "pbr-local");

    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");
    git(&dir, &["push"]);

    write(&dir, "b.txt", "world");
    commit_all(&dir, "second");

    let repo = Repo::open(&dir).unwrap();
    let outcome = repo.push().unwrap();
    assert_eq!(outcome, gitcore::PushOutcome::Success);

    // 验证 remote 收到了
    let remote_log = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&remote)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&remote_log.stdout);
    assert!(log.contains("second"), "remote 应收到 second 提交");

    cleanup(&[&remote, &dir]);
}

#[test]
fn push_without_upstream_returns_no_upstream() {
    let dir = init_repo("pnu");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();
    let outcome = repo.push().unwrap();
    assert_eq!(outcome, gitcore::PushOutcome::NoUpstream);

    cleanup(&[&dir]);
}

#[test]
fn push_non_fast_forward_rejected() {
    let remote = bare_remote("pnff");
    let a = clone(&remote, "pnff-a");

    // 初始提交(让 remote 有内容)
    write(&a, "init.txt", "init");
    commit_all(&a, "init");
    git(&a, &["push", "-u", "origin", "main"]);

    let b = clone(&remote, "pnff-b");

    // a 再提交并推送
    write(&a, "a.txt", "from-a");
    commit_all(&a, "a-change");
    git(&a, &["push"]);

    // b 本地提交(不知道 a 的改动)
    write(&b, "b.txt", "from-b");
    commit_all(&b, "b-change");

    let repo = Repo::open(&b).unwrap();
    let outcome = repo.push().unwrap();
    assert_eq!(
        outcome,
        gitcore::PushOutcome::NonFastForward,
        "远端领先时 push 应被拒"
    );

    cleanup(&[&remote, &a, &b]);
}

// ========== fetch streaming(进度 / 取消)测试 ==========

#[test]
fn fetch_streaming_updates_remote_tracking() {
    use gitcore::{CancelToken, Progress};
    // a 推到裸库;b 克隆后 a 再推一提交 → fetch 后 b 应落后 1。
    let a = init_repo("fs-a");
    write(&a, "f.txt", "1");
    commit_all(&a, "c1");
    let remote = bare_remote("fs-remote");
    git(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&a, &["push", "-q", "-u", "origin", "main"]);

    let b = clone(&remote, "fs-b");

    write(&a, "f.txt", "2");
    commit_all(&a, "c2");
    git(&a, &["push", "-q", "origin", "main"]);

    let repo = Repo::open(&b).unwrap();
    let cancel = CancelToken::default();
    let mut cb = |_p: Progress| {};
    repo.fetch_streaming(&mut cb, &cancel).unwrap();

    // fetch 已更新 origin/main 到 c2,故应检测到落后 1。
    assert_eq!(repo.status().unwrap().behind, 1, "fetch 后应落后 1");

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn fetch_streaming_honors_precancelled_token() {
    use gitcore::{CancelToken, Error, Progress};
    let remote = bare_remote("fsc-remote");
    let b = clone(&remote, "fsc-b");

    let repo = Repo::open(&b).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel(); // 进 fetch 前就取消:读循环第一轮即中止,与 fetch 快慢无关。
    let mut cb = |_p: Progress| {};
    let r = repo.fetch_streaming(&mut cb, &cancel);
    assert!(
        matches!(r, Err(Error::Cancelled)),
        "预置取消应返回 Cancelled,实际 {r:?}"
    );

    cleanup(&[&remote, &b]);
}

#[test]
fn push_streaming_succeeds_to_bare_remote() {
    use gitcore::{CancelToken, Progress};
    let remote = bare_remote("pss-remote");
    let dir = clone(&remote, "pss-local");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");
    git(&dir, &["push", "-q", "-u", "origin", "main"]); // 建立 upstream

    write(&dir, "b.txt", "world");
    commit_all(&dir, "second");

    let repo = Repo::open(&dir).unwrap();
    let cancel = CancelToken::default();
    let mut cb = |_p: Progress| {};
    assert_eq!(
        repo.push_streaming(&mut cb, &cancel).unwrap(),
        gitcore::PushOutcome::Success
    );

    // 验证 remote 真收到了 second。
    let log = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&remote)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("second"),
        "remote 应收到 second 提交"
    );

    cleanup(&[&remote, &dir]);
}

// 预置取消应在 fetch 阶段(autostash 之前)就中止 update,不产生任何 stash。
#[test]
fn update_honors_precancelled_token_before_autostash() {
    let (a, remote, b) = setup_divergent_no_conflict("ucancel");
    // 弄脏工作区:若取消晚到(autostash 之后才生效),会残留 stash —— 用它反证取消够早。
    write(&b, "dirty.txt", "WIP\n");

    let repo = Repo::open(&b).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel(); // 进 fetch 前取消:fetch 读循环第一轮即中止,早于 autostash。
    let r = repo.execute_update(&UpdateOptions::default(), &cancel);
    assert!(
        matches!(r, Err(gitcore::Error::Cancelled)),
        "预置取消应返回 Cancelled,实际 {r:?}"
    );

    // 取消发生在 autostash 之前:不应残留 stash,脏改动仍在工作区。
    assert!(repo.stashes().unwrap().is_empty(), "取消不应残留 autostash");
    assert_eq!(
        std::fs::read_to_string(b.join("dirty.txt")).unwrap(),
        "WIP\n"
    );

    cleanup(&[&a, &remote, &b]);
}

#[test]
fn commit_amend_rewrites_head_without_new_commit() {
    let dir = init_repo("amend");
    write(&dir, "a.txt", "hello");
    commit_all(&dir, "init");

    let repo = Repo::open(&dir).unwrap();

    // 正常提交 b.txt
    write(&dir, "b.txt", "world");
    repo.stage(&[Path::new("b.txt")]).unwrap();
    repo.commit(&gitcore::CommitOptions {
        message: "add b".into(),
        allow_empty: false,
        amend: false,
        no_verify: false,
    })
    .unwrap();

    let rev_count = |d: &Path| -> usize {
        let out = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(d)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
    };
    let before = rev_count(&dir);

    // amend:无新暂存改动,仅改 message —— 验证 amend 跳过"暂存区为空"检查
    repo.commit(&gitcore::CommitOptions {
        message: "add b (reworded)".into(),
        allow_empty: false,
        amend: true,
        no_verify: false,
    })
    .unwrap();

    // 提交数不变(替换而非新增)
    assert_eq!(rev_count(&dir), before, "amend 不应增加提交数");

    // 顶部 message 已改写
    let msg = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&msg.stdout).trim(),
        "add b (reworded)",
        "amend 应改写顶部提交 message"
    );

    cleanup(&[&dir]);
}

#[test]
fn blame_attributes_lines_to_commits() {
    let dir = init_repo("blame");
    write(&dir, "f.txt", "line1\nline2\n");
    commit_all(&dir, "first");
    // 只改第二行,使两行归属不同提交
    write(&dir, "f.txt", "line1\nline2-changed\n");
    commit_all(&dir, "second");

    let repo = Repo::open(&dir).unwrap();
    let blame = repo.blame(Path::new("f.txt")).unwrap();

    assert_eq!(blame.len(), 2, "应有 2 行 blame");
    assert_eq!(blame[0].line_no, 1);
    assert_eq!(blame[0].content, "line1");
    assert_eq!(blame[1].line_no, 2);
    assert_eq!(blame[1].content, "line2-changed");
    assert_ne!(blame[0].full_sha, blame[1].full_sha, "两行应来自不同提交");
    assert!(!blame[0].author.is_empty(), "应解析出作者");
    assert!(blame[0].time > 0, "应解析出时间戳");

    cleanup(&[&dir]);
}

// ── smart checkout(脏工作区智能切换)──

fn head_branch(dir: &PathBuf) -> String {
    Repo::open(dir).unwrap().status().unwrap().branch.unwrap()
}

#[test]
fn smart_switch_clean_worktree_switches() {
    let dir = init_repo("sc-clean");
    write(&dir, "a.txt", "base");
    commit_all(&dir, "init");
    git(&dir, &["branch", "feat"]);

    let r = Repo::open(&dir).unwrap().switch_branch_autostash("feat");
    assert!(matches!(r, Ok(SwitchOutcome::Switched)), "干净应直接切换");
    assert_eq!(head_branch(&dir), "feat");

    cleanup(&[&dir]);
}

#[test]
fn smart_switch_dirty_nonconflicting_carries_change() {
    // a.txt 两分支相同、b.txt 不同;脏改动落在 a.txt → 贴回不冲突。
    let dir = init_repo("sc-carry");
    write(&dir, "a.txt", "base");
    write(&dir, "b.txt", "b-main");
    commit_all(&dir, "init");
    git(&dir, &["checkout", "-q", "-b", "feat"]);
    write(&dir, "b.txt", "b-feat");
    commit_all(&dir, "feat changes b");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "a.txt", "dirty-a"); // 脏:改 a.txt(两分支一致,贴回安全)

    let r = Repo::open(&dir).unwrap().switch_branch_autostash("feat");
    assert!(
        matches!(r, Ok(SwitchOutcome::Switched)),
        "无冲突应 Switched,实际 {r:?}"
    );
    assert_eq!(head_branch(&dir), "feat");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "dirty-a",
        "脏改动应贴回新分支"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("b.txt")).unwrap(),
        "b-feat",
        "新分支自身的内容应生效"
    );

    cleanup(&[&dir]);
}

#[test]
fn smart_switch_dirty_conflicting_reports_stash_conflict() {
    // a.txt 在 feat 与脏改动各不相同 → 贴回冲突。
    let dir = init_repo("sc-conflict");
    write(&dir, "a.txt", "base");
    commit_all(&dir, "init");
    git(&dir, &["checkout", "-q", "-b", "feat"]);
    write(&dir, "a.txt", "feat-version");
    commit_all(&dir, "feat changes a");
    git(&dir, &["checkout", "-q", "main"]);
    write(&dir, "a.txt", "my-dirty"); // 脏:与 feat 的 a.txt 冲突

    let r = Repo::open(&dir).unwrap().switch_branch_autostash("feat");
    match r {
        Ok(SwitchOutcome::StashConflict { files }) => {
            assert!(
                files.iter().any(|f| f.ends_with("a.txt")),
                "冲突文件应含 a.txt,实际 {files:?}"
            );
        }
        other => panic!("贴回应冲突,实际 {other:?}"),
    }
    assert_eq!(head_branch(&dir), "feat", "已切到 feat(checkout 成功)");
    let content = std::fs::read_to_string(dir.join("a.txt")).unwrap();
    assert!(content.contains("<<<<<<<"), "a.txt 应留有冲突标记");
    // stash 仍保留(贴回失败不丢改动)
    let stash_list = Repo::open(&dir).unwrap().stashes().unwrap();
    assert!(!stash_list.is_empty(), "贴回冲突时 stash 应保留");

    cleanup(&[&dir]);
}

#[test]
fn stash_push_list_apply_drop_roundtrip() {
    let dir = init_repo("stash-rt");
    write(&dir, "a.txt", "base");
    commit_all(&dir, "init");
    write(&dir, "a.txt", "changed"); // 脏

    let repo = Repo::open(&dir).unwrap();
    repo.stash_push(Some("my wip")).unwrap();
    assert!(!repo.status().unwrap().dirty, "stash 后工作区应干净");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "base",
        "stash 后内容回到已提交版本"
    );

    let list = repo.stashes().unwrap();
    assert_eq!(list.len(), 1, "应有 1 条 stash");
    assert!(
        list[0].message.contains("my wip"),
        "应含自定义说明,实际 {:?}",
        list[0].message
    );

    // apply 保留 stash
    repo.stash_apply(&list[0].reff).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "changed",
        "apply 后改动回到工作区"
    );
    assert_eq!(repo.stashes().unwrap().len(), 1, "apply 保留 stash");

    // drop 删除
    repo.stash_drop(&list[0].reff).unwrap();
    assert_eq!(repo.stashes().unwrap().len(), 0, "drop 后无 stash");

    cleanup(&[&dir]);
}

#[test]
fn reset_mixed_keeps_worktree_hard_discards() {
    use gitcore::ResetMode;
    let dir = init_repo("reset");
    write(&dir, "a.txt", "v1\n");
    commit_all(&dir, "c1");
    let c1 = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dir)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    write(&dir, "a.txt", "v2\n");
    commit_all(&dir, "c2");

    let repo = Repo::open(&dir).unwrap();
    // mixed:HEAD 回到 c1,工作区文件仍是 c2 内容(v2)→ 相对 c1 为脏
    repo.reset(&c1, ResetMode::Mixed).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "v2\n",
        "mixed 不动工作区"
    );
    assert!(repo.status().unwrap().dirty, "mixed 后工作区应为脏");

    // hard:回到 c1 并丢弃工作区改动 → 文件变回 v1,干净
    repo.reset(&c1, ResetMode::Hard).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "v1\n",
        "hard 丢弃改动回到 c1"
    );
    assert!(!repo.status().unwrap().dirty, "hard 后工作区干净");

    cleanup(&[&dir]);
}

#[test]
fn tag_create_list_delete_roundtrip() {
    let dir = init_repo("tag");
    write(&dir, "a.txt", "v1\n");
    commit_all(&dir, "c1");

    let repo = Repo::open(&dir).unwrap();
    // 轻量标签(在 HEAD)
    repo.create_tag("v1.0", None, None).unwrap();
    // 注释标签(指定提交 = HEAD)
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dir)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    repo.create_tag("v1.0-ann", Some(&head), Some("release one"))
        .unwrap();

    let tags = repo.tags().unwrap();
    assert_eq!(tags.len(), 2, "应有 2 个 tag");
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"v1.0"), "含轻量标签");
    assert!(names.contains(&"v1.0-ann"), "含注释标签");
    let ann = tags.iter().find(|t| t.name == "v1.0-ann").unwrap();
    assert_eq!(ann.message, "release one", "注释标签 message 为注释主题");

    repo.delete_tag("v1.0").unwrap();
    assert_eq!(repo.tags().unwrap().len(), 1, "删除后剩 1 个");

    cleanup(&[&dir]);
}

// ── 交互式变基(Interactively Rebase from Here) ──

fn head_sha(dir: &PathBuf) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn head_subject(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn rev_count(dir: &Path) -> usize {
    let out = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn rebase_plan_lists_from_commit_to_head_oldest_first() {
    let dir = init_repo("rb-plan");
    write(&dir, "a.txt", "a\n");
    commit_all(&dir, "A");
    write(&dir, "b.txt", "b\n");
    commit_all(&dir, "B");
    let b = head_sha(&dir);
    write(&dir, "c.txt", "c\n");
    commit_all(&dir, "C");
    write(&dir, "d.txt", "d\n");
    commit_all(&dir, "D");

    let repo = Repo::open(&dir).unwrap();
    let plan = repo.rebase_plan(&b).unwrap();
    let msgs: Vec<&str> = plan.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(
        msgs,
        ["B", "C", "D"],
        "应含 from_sha 起到 HEAD,oldest-first"
    );

    cleanup(&[&dir]);
}

#[test]
fn rebase_interactive_reword_squash_drop() {
    let dir = init_repo("rb-int");
    write(&dir, "a.txt", "a\n");
    commit_all(&dir, "A");
    write(&dir, "b.txt", "b\n");
    commit_all(&dir, "B");
    let b = head_sha(&dir);
    write(&dir, "c.txt", "c\n");
    commit_all(&dir, "C");
    write(&dir, "d.txt", "d\n");
    commit_all(&dir, "D");

    let repo = Repo::open(&dir).unwrap();
    let plan = repo.rebase_plan(&b).unwrap(); // [B, C, D]
    let items = vec![
        RebaseItem {
            sha: plan[0].full_sha.clone(),
            action: RebaseAction::Reword("B2".into()),
        },
        RebaseItem {
            sha: plan[1].full_sha.clone(),
            action: RebaseAction::Squash("BC".into()),
        },
        RebaseItem {
            sha: plan[2].full_sha.clone(),
            action: RebaseAction::Drop,
        },
    ];
    let outcome = repo.rebase_interactive(&b, &items).unwrap();
    assert!(
        matches!(outcome, UpdateOutcome::Resolved),
        "无冲突应 Resolved,实际 {outcome:?}"
    );

    // A + (B 折叠 C) = 2 个提交;D 丢弃。
    assert_eq!(rev_count(&dir), 2, "drop D、squash 折叠 C 后应剩 A + BC");
    assert_eq!(
        head_subject(&dir),
        "BC",
        "squash 的信息成为合并后提交 subject"
    );
    assert!(dir.join("b.txt").exists(), "B 保留");
    assert!(dir.join("c.txt").exists(), "C 的改动被折叠保留");
    assert!(!dir.join("d.txt").exists(), "D 被丢弃");

    cleanup(&[&dir]);
}

#[test]
fn rebase_interactive_requires_clean_worktree() {
    let dir = init_repo("rb-dirty");
    write(&dir, "a.txt", "a\n");
    commit_all(&dir, "A");
    let a = head_sha(&dir);
    write(&dir, "b.txt", "b\n");
    commit_all(&dir, "B");
    // 制造脏工作区
    write(&dir, "a.txt", "a-changed\n");

    let repo = Repo::open(&dir).unwrap();
    let items = vec![RebaseItem {
        sha: a.clone(),
        action: RebaseAction::Reword("A2".into()),
    }];
    assert!(
        repo.rebase_interactive(&a, &items).is_err(),
        "脏工作区应被拒绝"
    );

    cleanup(&[&dir]);
}

#[test]
fn rebase_interactive_sequential_conflicts_resolve_through() {
    // 三个提交都改 f.txt 同一行;把 [B, C] 重排为 [C, B] → 顺序两次冲突,
    // 直击 continue_update 新增的"下一个提交又冲突"分支(交互式变基最危险路径)。
    let dir = init_repo("rb-seq");
    write(&dir, "f.txt", "A\n");
    commit_all(&dir, "A");
    write(&dir, "f.txt", "B\n");
    commit_all(&dir, "B");
    let b = head_sha(&dir);
    write(&dir, "f.txt", "C\n");
    commit_all(&dir, "C");

    let repo = Repo::open(&dir).unwrap();
    let plan = repo.rebase_plan(&b).unwrap(); // [B, C]
    let items = vec![
        RebaseItem {
            sha: plan[1].full_sha.clone(),
            action: RebaseAction::Pick,
        }, // C 先
        RebaseItem {
            sha: plan[0].full_sha.clone(),
            action: RebaseAction::Pick,
        }, // 再 B
    ];

    // 冲突 #1:C 应用到 A 上冲突。
    let o1 = repo.rebase_interactive(&b, &items).unwrap();
    assert!(
        matches!(o1, UpdateOutcome::Conflicted { .. }),
        "重排后首个提交应冲突,实际 {o1:?}"
    );
    write(&dir, "f.txt", "C\n");
    git(&dir, &["add", "f.txt"]);

    // 冲突 #2:continue 推进到 B,又冲突(验证 continue_update 顺序冲突分支)。
    let o2 = repo.continue_update(None, false).unwrap();
    assert!(
        matches!(o2, UpdateOutcome::Conflicted { .. }),
        "续跑到第二个提交应再次冲突,实际 {o2:?}"
    );
    write(&dir, "f.txt", "B\n");
    git(&dir, &["add", "f.txt"]);

    // 二次解决后完成。
    let o3 = repo.continue_update(None, false).unwrap();
    assert!(
        matches!(o3, UpdateOutcome::Resolved),
        "二次解决后应完成,实际 {o3:?}"
    );
    assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "B\n");

    cleanup(&[&dir]);
}

// ── reflog 查看 ──

#[test]
fn reflog_lists_head_movements_newest_first() {
    let dir = init_repo("reflog");
    write(&dir, "f.txt", "A\n");
    commit_all(&dir, "A");
    write(&dir, "f.txt", "B\n");
    commit_all(&dir, "B");
    let b = head_sha(&dir);

    let repo = Repo::open(&dir).unwrap();
    let entries = repo.reflog(50).unwrap();
    assert!(entries.len() >= 2, "至少 A、B 两次提交的 reflog");
    assert_eq!(entries[0].selector, "HEAD@{0}", "最新在前");
    assert_eq!(entries[0].full_sha, b, "顶部指向最新提交 B");
    assert!(
        entries[0].action.contains("commit"),
        "最近动作应是 commit,实际 {:?}",
        entries[0].action
    );

    cleanup(&[&dir]);
}

// ── 推送 tag 到远程 ──

#[test]
fn push_tag_publishes_to_remote() {
    let work = init_repo("pushtag");
    write(&work, "f.txt", "1\n");
    commit_all(&work, "c1");
    let remote = bare_remote("pushtag-remote");
    git(
        &work,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&work, &["push", "-q", "-u", "origin", "main"]);

    let repo = Repo::open(&work).unwrap();
    repo.create_tag("v1.0", None, None).unwrap();
    repo.push_tag("v1.0").unwrap();

    // 远程裸库应已收到该 tag。
    let out = Command::new("git")
        .args(["tag", "--list"])
        .current_dir(&remote)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("v1.0"),
        "push_tag 后远程应有 v1.0"
    );

    cleanup(&[&work, &remote]);
}
