use std::path::{Path, PathBuf};

/// 仓库配置项。
#[derive(Debug, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub path: PathBuf,
}

/// 解析 .git-gui/repos.toml 配置文件。
pub fn parse_repos_config(config_path: &Path) -> Result<Vec<RepoConfig>, String> {
    let content =
        std::fs::read_to_string(config_path).map_err(|e| format!("读取配置文件失败: {e}"))?;

    parse_toml(&content)
}

/// 简单 toml 解析器（只支持 [[repos]] name/path）。
fn parse_toml(content: &str) -> Result<Vec<RepoConfig>, String> {
    let mut repos = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_path: Option<PathBuf> = None;

    for line in content.lines() {
        let line = line.trim();

        // 跳过空行和注释
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // [[repos]] 开始新的 repo 块
        if line == "[[repos]]" {
            // 保存上一个 repo
            if let (Some(name), Some(path)) = (current_name.take(), current_path.take()) {
                repos.push(RepoConfig { name, path });
            }
            continue;
        }

        // name = "..."
        if let Some(rest) = line.strip_prefix("name") {
            if let Some(value) = extract_string_value(rest) {
                current_name = Some(value);
            }
        }

        // path = "..."
        if let Some(rest) = line.strip_prefix("path") {
            if let Some(value) = extract_string_value(rest) {
                current_path = Some(PathBuf::from(value));
            }
        }
    }

    // 保存最后一个 repo
    if let (Some(name), Some(path)) = (current_name, current_path) {
        repos.push(RepoConfig { name, path });
    }

    Ok(repos)
}

/// 提取 key = "value" 中的 value。
fn extract_string_value(rest: &str) -> Option<String> {
    let rest = rest.trim();
    if !rest.starts_with('=') {
        return None;
    }
    let rest = rest[1..].trim();
    if !rest.starts_with('"') {
        return None;
    }
    let rest = &rest[1..];
    rest.find('"').map(|end| rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_config() {
        let toml = r#"
# 注释
[[repos]]
name = "主仓库"
path = "."

[[repos]]
name = "前端"
path = "../frontend"
"#;
        let repos = parse_toml(toml).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "主仓库");
        assert_eq!(repos[0].path, PathBuf::from("."));
        assert_eq!(repos[1].name, "前端");
        assert_eq!(repos[1].path, PathBuf::from("../frontend"));
    }
}
