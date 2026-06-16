//! gitcore 集成测试:在临时 git 仓库上验证真实行为。
//! 每个测试自建临时 repo、用完即删。

use gitcore::{Repo, UpdateOptions, UpdateOutcome};
use std::path::PathBuf;
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

fn clone(remote: &PathBuf, tag: &str) -> PathBuf {
    let dir = unique_dir(tag);
    git(&dir, &["clone", "-q", remote.to_str().unwrap(), "."]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    dir
}

fn write(dir: &PathBuf, name: &str, content: &str) {
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
        .execute_update(&UpdateOptions::default())
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

    let (files, autostash) = match repo.execute_update(&UpdateOptions::default()).unwrap() {
        UpdateOutcome::Conflicted { files, autostash } => (files, autostash),
        other => panic!("应当冲突,实际 {other:?}"),
    };
    assert_eq!(files, vec![PathBuf::from("f.txt")]);

    let segs = repo.read_conflict(&files[0]).unwrap();
    let hunks: Vec<_> = segs
        .iter()
        .filter_map(|s| match s {
            gitcore::Segment::Conflict(h) => Some(h),
            gitcore::Segment::Clean(_) => None,
        })
        .collect();
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].magic(), gitcore::Resolution::NeedsUser);

    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo.resolve_file(&files[0], &resolved).unwrap();

    assert!(matches!(
        repo.continue_update(autostash).unwrap(),
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

    let autostash = match repo.execute_update(&UpdateOptions::default()).unwrap() {
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
    let out = repo.execute_update(&UpdateOptions::default()).unwrap();
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
    let (files, autostash) = repo2
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
        repo2.continue_update(autostash).unwrap(),
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
    let (files, autostash) = match repo.execute_update(&UpdateOptions::default()).unwrap() {
        UpdateOutcome::Conflicted { files, autostash } => (files, autostash),
        o => panic!("应冲突,实际 {o:?}"),
    };
    let segs = repo.read_conflict(&files[0]).unwrap();
    let resolved = gitcore::rebuild(&segs, &[gitcore::Choice::Theirs]);
    repo.resolve_file(&files[0], &resolved).unwrap();
    repo.continue_update(autostash).unwrap();

    // 撤销这次 merge,回到冲突前。
    git(&b, &["reset", "--hard", "HEAD~1"]);

    // 第二次:同样冲突 → rerere 重放 + 自动确认 → 无需人工,直接完成。
    match repo.execute_update(&UpdateOptions::default()).unwrap() {
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
