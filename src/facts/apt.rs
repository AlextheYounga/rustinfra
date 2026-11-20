use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;

const FILE_MARKER: &str = "##PYINFRA_FILE ";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AptRepo {
    pub repository_type: String,
    pub url: String,
    pub distribution: String,
    pub components: Vec<String>,
    pub options: IndexMap<String, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AptSources;

impl Fact for AptSources {
    type Input = ();
    type Output = Vec<AptRepo>;

    fn name(&self) -> &'static str {
        "apt.AptSources"
    }

    fn command(&self, (): &Self::Input) -> StringCommand {
        StringCommand::new([
            "sh -c 'for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do [ -e \"$f\" ] || continue; echo \"##PYINFRA_FILE $f\"; cat \"$f\"; echo; done'",
        ])
    }

    fn default(&self) -> Self::Output {
        Vec::new()
    }

    fn requires_command(&self, (): &Self::Input) -> Option<&str> {
        Some("apt")
    }

    fn process(&self, output: &[String], (): &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut repositories = Vec::new();
        let mut current_file = None;
        let mut buffer = Vec::new();
        for line in output {
            if let Some(path) = line.strip_prefix(FILE_MARKER) {
                parse_file(current_file.as_deref(), &buffer, &mut repositories);
                current_file = Some(path.trim().to_owned());
                buffer.clear();
            } else {
                buffer.push(line.clone());
            }
        }
        parse_file(current_file.as_deref(), &buffer, &mut repositories);
        Ok(repositories)
    }
}

pub fn parse_apt_repo(line: &str) -> Option<AptRepo> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split_whitespace();
    let repository_type = fields.next()?;
    if !matches!(repository_type, "deb" | "deb-src") {
        return None;
    }
    let mut options = IndexMap::new();
    let mut next = fields.next()?;
    if next.starts_with('[') {
        let mut option_text = next.trim_start_matches('[').to_owned();
        while !next.ends_with(']') {
            next = fields.next()?;
            option_text.push(' ');
            option_text.push_str(next);
        }
        for option in option_text.trim_end_matches(']').split_whitespace() {
            if let Some((key, values)) = option.split_once('=') {
                options.insert(key.to_owned(), values.split(',').map(str::to_owned).collect());
            }
        }
        next = fields.next()?;
    }
    let url = next.to_owned();
    let distribution = fields.next()?.to_owned();
    let components = fields.map(str::to_owned).collect();
    Some(AptRepo { repository_type: repository_type.to_owned(), url, distribution, components, options })
}

fn parse_file(path: Option<&str>, lines: &[String], repositories: &mut Vec<AptRepo>) {
    let Some(path) = path else {
        return;
    };
    if path.ends_with(".sources") {
        parse_deb822(lines, repositories);
    } else {
        repositories.extend(lines.iter().filter_map(|line| parse_apt_repo(line)));
    }
}

fn parse_deb822(lines: &[String], repositories: &mut Vec<AptRepo>) {
    let mut stanza = Vec::new();
    for line in lines.iter().map(String::as_str).chain([""]) {
        if line.trim().is_empty() {
            if !stanza.is_empty() {
                expand_deb822(&stanza, repositories);
                stanza.clear();
            }
        } else {
            stanza.push(line);
        }
    }
}

fn expand_deb822(lines: &[&str], repositories: &mut Vec<AptRepo>) {
    let mut fields = IndexMap::<String, String>::new();
    let mut current_key: Option<String> = None;
    for line in lines {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(key) = &current_key {
                fields.entry(key.clone()).and_modify(|value| {
                    value.push('\n');
                    value.push_str(line[1..].trim_end());
                });
            }
        } else if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_owned();
            fields.insert(key.clone(), value.trim().to_owned());
            current_key = Some(key);
        }
    }
    if fields.get("Enabled").is_some_and(|enabled| !enabled.eq_ignore_ascii_case("yes")) {
        return;
    }
    let (Some(types), Some(urls), Some(suites)) = (fields.get("Types"), fields.get("URIs"), fields.get("Suites"))
    else {
        return;
    };
    let components = split_field(fields.get("Components"));
    let mut options = IndexMap::new();
    for (field, option) in [("Architectures", "arch"), ("Signed-By", "signed-by"), ("Trusted", "trusted")] {
        if let Some(value) = fields.get(field) {
            options.insert(option.to_owned(), value.split_whitespace().map(str::to_owned).collect());
        }
    }
    for repository_type in types.split_whitespace() {
        for url in urls.split_whitespace() {
            for distribution in suites.split_whitespace() {
                repositories.push(AptRepo {
                    repository_type: repository_type.to_owned(),
                    url: url.to_owned(),
                    distribution: distribution.to_owned(),
                    components: components.clone(),
                    options: options.clone(),
                });
            }
        }
    }
}

fn split_field(value: Option<&String>) -> Vec<String> {
    value.into_iter().flat_map(|value| value.split_whitespace()).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::{AptSources, Fact, parse_apt_repo};

    #[test]
    fn parses_legacy_options_and_deb822_combinations() {
        let legacy =
            parse_apt_repo("deb [arch=amd64,arm64 signed-by=/key.gpg] https://example stable main contrib").unwrap();
        assert_eq!(legacy.options["arch"], ["amd64", "arm64"]);
        assert_eq!(legacy.components, ["main", "contrib"]);

        let output = [
            "##PYINFRA_FILE /etc/apt/sources.list.d/vendor.sources".to_owned(),
            "Types: deb deb-src".to_owned(),
            "URIs: https://one https://two".to_owned(),
            "Suites: stable".to_owned(),
            "Components: main".to_owned(),
            "Signed-By: /key.gpg".to_owned(),
        ];
        let repositories = AptSources.process(&output, &()).unwrap();
        assert_eq!(repositories.len(), 4);
        assert_eq!(repositories[0].options["signed-by"], ["/key.gpg"]);
    }
}
