use std::env;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use glob::glob;
use ssh2_config::{HostParams, ParseRule, SshConfig};

pub(super) const HOME_ENVIRONMENT_VARIABLE: &str = "HOME";

pub(super) fn resolve(host: &str, configured_path: Option<&str>) -> Result<Option<HostParams>, String> {
    let Some(path) = config_path(configured_path) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let expanded = expand_includes(&path, &mut Vec::new())?;
    let mut reader = Cursor::new(expanded.into_bytes());
    let rules = ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS;
    let config = SshConfig::default()
        .parse(&mut reader, rules)
        .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
    Ok(Some(config.query(host)))
}

fn config_path(configured_path: Option<&str>) -> Option<PathBuf> {
    match configured_path {
        Some("") => None,
        Some(path) => Some(expand_user(path)),
        None => env::var_os(HOME_ENVIRONMENT_VARIABLE).map(PathBuf::from).map(|home| home.join(".ssh/config")),
    }
}

fn expand_user(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = env::var_os(HOME_ENVIRONMENT_VARIABLE)
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

fn expand_includes(path: &Path, stack: &mut Vec<PathBuf>) -> Result<String, String> {
    let path = path.to_path_buf();
    if stack.contains(&path) {
        return Err(format!("include loop detected in SSH config file: {}", path.display()));
    }
    stack.push(path.clone());
    let contents = fs::read_to_string(&path).map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    let mut expanded = String::new();

    for raw_line in contents.lines() {
        let line = strip_inline_comment(raw_line.trim());
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = setting(line) else {
            expanded.push_str(line);
            expanded.push('\n');
            continue;
        };
        if !key.eq_ignore_ascii_case("include") {
            expanded.push_str(line);
            expanded.push('\n');
            continue;
        }

        let include = expand_user(value);
        let include =
            if include.is_absolute() { include } else { path.parent().unwrap_or_else(|| Path::new("")).join(include) };
        let pattern =
            include.to_str().ok_or_else(|| format!("SSH config include path is not UTF-8: {}", include.display()))?;
        let matches = glob(pattern).map_err(|error| format!("invalid SSH config include `{pattern}`: {error}"))?;
        for included in matches {
            let included = included.map_err(|error| format!("unable to read SSH config include: {error}"))?;
            if included.is_file() {
                expanded.push_str(&expand_includes(&included, stack)?);
            }
        }
    }
    stack.pop();
    Ok(expanded)
}

fn setting(line: &str) -> Option<(&str, &str)> {
    let separator = line.find(|character: char| character.is_whitespace() || character == '=')?;
    let key = &line[..separator];
    let value = line[separator..].trim_start().strip_prefix('=').unwrap_or(&line[separator..]).trim();
    Some((key, value))
}

fn strip_inline_comment(line: &str) -> &str {
    let mut quote = None;
    let mut previous_whitespace = false;
    for (index, character) in line.char_indices() {
        match (quote, character) {
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), close) if open == close => quote = None,
            (None, '#') if previous_whitespace => return line[..index].trim_end(),
            _ => {}
        }
        previous_whitespace = character.is_whitespace();
    }
    line
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::resolve;

    #[test]
    fn resolves_aliases_and_relative_includes_with_first_value_wins() {
        let directory = tempdir().unwrap();
        let included = directory.path().join("hosts.conf");
        fs::write(
            &included,
            "Host alias\n  HostName target.example\n  User config-user\n  Port 2202\n  IdentityFile ~/.ssh/config-key\n",
        )
        .unwrap();
        let config = directory.path().join("config");
        fs::write(&config, "Include hosts.conf # relative include\nHost *\n  Port 22\n").unwrap();

        let resolved = resolve("alias", config.to_str()).unwrap().unwrap();

        assert_eq!(resolved.host_name.as_deref(), Some("target.example"));
        assert_eq!(resolved.user.as_deref(), Some("config-user"));
        assert_eq!(resolved.port, Some(2202));
        assert_eq!(resolved.identity_file.unwrap().len(), 1);
    }

    #[test]
    fn detects_recursive_includes() {
        let directory = tempdir().unwrap();
        let config = directory.path().join("config");
        fs::write(&config, "Include config\n").unwrap();

        assert!(resolve("host", config.to_str()).unwrap_err().contains("include loop"));
    }
}
