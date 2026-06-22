use crate::{Error, Repo};

/// 一个 tag。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct TagInfo {
    pub name: String,
    /// 指向的提交短 sha(注释标签解引用到提交)。
    pub target: String,
    /// 注释标签的消息主题;轻量标签为所指提交的主题。
    pub message: String,
}

/// 列出所有 tag(按创建时间倒序)。
pub(crate) fn list_tags(repo: &Repo) -> Result<Vec<TagInfo>, Error> {
    let output = repo.git(&[
        "for-each-ref",
        "--sort=-creatordate",
        "--format=%(refname:short)%00%(objectname:short)%00%(*objectname:short)%00%(contents:subject)",
        "refs/tags",
    ])?;
    let mut tags = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 4 {
            continue;
        }
        // 注释标签:objectname 是 tag 对象,*objectname 才是提交;轻量标签只有 objectname。
        let target = if parts[2].is_empty() {
            parts[1].to_string()
        } else {
            parts[2].to_string()
        };
        tags.push(TagInfo {
            name: parts[0].to_string(),
            target,
            message: parts[3].to_string(),
        });
    }
    Ok(tags)
}

/// 创建 tag。`target` 为 None 时打在 HEAD;`message` 为 Some 时创建注释标签,否则轻量标签。
pub(crate) fn create_tag(
    repo: &Repo,
    name: &str,
    target: Option<&str>,
    message: Option<&str>,
) -> Result<(), Error> {
    let mut args = vec!["tag"];
    if let Some(m) = message {
        args.push("-a");
        args.push("-m");
        args.push(m);
    }
    args.push(name);
    if let Some(t) = target {
        args.push(t);
    }
    repo.git(&args)?;
    Ok(())
}

/// 删除 tag。
pub(crate) fn delete_tag(repo: &Repo, name: &str) -> Result<(), Error> {
    repo.git(&["tag", "-d", name])?;
    Ok(())
}
