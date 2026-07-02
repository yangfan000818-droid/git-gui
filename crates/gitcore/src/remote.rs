use crate::{Error, Repo};

/// 一个远程仓库。
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct RemoteInfo {
    pub name: String,
    /// fetch URL。
    pub url: String,
    /// push URL(未单独设置时与 url 相同)。
    pub push_url: String,
}

/// 列出所有远程仓库(名称 + fetch/push URL)。
pub(crate) fn list_remotes(repo: &Repo) -> Result<Vec<RemoteInfo>, Error> {
    // `git remote -v` 每个 remote 输出两行:"<name>\t<url> (fetch)" / "(push)"。
    let out = repo.git(&["remote", "-v"])?;
    let mut remotes: Vec<RemoteInfo> = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        // rest = "<url> (fetch)" 或 "<url> (push)"。
        let is_push = rest.ends_with("(push)");
        let url = rest
            .rsplit_once(' ')
            .map(|(u, _)| u.trim())
            .unwrap_or(rest.trim())
            .to_string();
        match remotes.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                if is_push {
                    r.push_url = url;
                }
            }
            None => remotes.push(RemoteInfo {
                name: name.to_string(),
                url: if is_push { String::new() } else { url.clone() },
                push_url: if is_push { url } else { String::new() },
            }),
        }
    }
    // fetch 行先出现,push 行补 push_url;若某侧为空用另一侧兜底。
    for r in &mut remotes {
        if r.url.is_empty() {
            r.url = r.push_url.clone();
        }
        if r.push_url.is_empty() {
            r.push_url = r.url.clone();
        }
    }
    Ok(remotes)
}

/// 添加一个远程仓库。
pub(crate) fn add_remote(repo: &Repo, name: &str, url: &str) -> Result<(), Error> {
    repo.git(&["remote", "add", name, url])?;
    Ok(())
}

/// 删除一个远程仓库。
pub(crate) fn remove_remote(repo: &Repo, name: &str) -> Result<(), Error> {
    repo.git(&["remote", "remove", name])?;
    Ok(())
}

/// 修改一个远程仓库的 URL。
pub(crate) fn set_remote_url(repo: &Repo, name: &str, url: &str) -> Result<(), Error> {
    repo.git(&["remote", "set-url", name, url])?;
    Ok(())
}
