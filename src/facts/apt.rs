use std::error::Error;
use std::io;

use indexmap::IndexMap;

use crate::api::{CommandBit, StringCommand};

use super::Fact;
use super::gpg::{GpgKeyInfo, GpgKeyrings};

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
                    let continuation = line[1..].trim_end();
                    if continuation != "." {
                        value.push_str(continuation);
                    }
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
            let values = if field == "Signed-By" && value.contains("-----BEGIN PGP") {
                vec![value.clone()]
            } else {
                value.split_whitespace().map(str::to_owned).collect()
            };
            options.insert(option.to_owned(), values);
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

#[derive(Clone, Copy, Debug, Default)]
pub struct AptPackages;
impl Fact for AptPackages {
    type Input = ();
    type Output = IndexMap<String, Vec<String>>;
    fn name(&self) -> &'static str {
        "apt.AptPackages"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["apt list --installed 2>/dev/null"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("apt")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = IndexMap::<String, Vec<String>>::new();
        for line in output {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 || !fields[3].starts_with("[installed") {
                continue;
            }
            let Some((name, _source)) = fields[0].split_once('/') else { continue };
            packages.entry(name.to_owned()).or_default().push(fields[1].to_owned());
        }
        Ok(packages)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AptPackageProvides;
impl Fact for AptPackageProvides {
    type Input = String;
    type Output = Vec<(String, String)>;
    fn name(&self) -> &'static str {
        "apt.AptPackageProvides"
    }
    fn command(&self, package: &String) -> StringCommand {
        StringCommand::new([CommandBit::from("apt-cache showpkg"), CommandBit::quoted(package.clone())])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("apt-cache")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut reverse = false;
        let mut packages = Vec::new();
        for line in output {
            let line = line.trim();
            if line == "Reverse Provides:" {
                reverse = true;
                continue;
            }
            if reverse && !line.is_empty() {
                let mut fields = line.split_whitespace();
                if let (Some(name), Some(version)) = (fields.next(), fields.next()) {
                    packages.push((name.to_owned(), version.to_owned()));
                }
            }
        }
        Ok(packages)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AptHeldPackages;
impl Fact for AptHeldPackages {
    type Input = ();
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "apt.AptHeldPackages"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["apt-mark showhold"])
    }
    fn default(&self) -> Vec<String> {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("apt-mark")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(output.iter().map(|line| line.trim()).filter(|line| !line.is_empty()).map(str::to_owned).collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AptKeys;
pub const APT_KEYRING_DIRECTORIES: [&str; 3] = ["/etc/apt/trusted.gpg.d", "/etc/apt/keyrings", "/usr/share/keyrings"];
impl Fact for AptKeys {
    type Input = Option<Vec<String>>;
    type Output = IndexMap<String, GpgKeyInfo>;
    fn name(&self) -> &'static str {
        "apt.AptKeys"
    }
    fn command(&self, directories: &Self::Input) -> StringCommand {
        let defaults;
        let directories = if let Some(directories) = directories {
            directories
        } else {
            defaults = APT_KEYRING_DIRECTORIES.iter().map(|value| (*value).to_owned()).collect();
            &defaults
        };
        GpgKeyrings.command(directories)
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &Self::Input) -> Option<&str> {
        Some("gpg")
    }
    fn process(&self, output: &[String], input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let keyrings = GpgKeyrings.process(output, input.as_ref().unwrap_or(&Vec::new()))?;
        Ok(keyrings.into_values().flat_map(|keyring| keyring.keys).collect())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AptSimulation {
    pub upgraded: u32,
    pub newly_installed: u32,
    pub removed: u32,
    pub not_upgraded: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SimulateOperationWillChange;
impl Fact for SimulateOperationWillChange {
    type Input = String;
    type Output = Option<AptSimulation>;
    fn name(&self) -> &'static str {
        "apt.SimulateOperationWillChange"
    }
    fn command(&self, action: &String) -> StringCommand {
        let action = StringCommand::new([CommandBit::quoted(action.clone())]).raw_value();
        StringCommand::new([format!(
            "LC_ALL=C DEBIAN_FRONTEND=noninteractive apt-get -y -o Dpkg::Options::=\"--force-confdef\" -o Dpkg::Options::=\"--force-confold\" {action} --dry-run"
        )])
    }
    fn default(&self) -> Self::Output {
        None
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("apt-get")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        for line in output {
            let values = line.split_whitespace().collect::<Vec<_>>();
            if values.len() >= 10 && values[1] == "upgraded," && values[4] == "installed," && values[7] == "remove" {
                return Ok(Some(AptSimulation {
                    upgraded: values[0].parse()?,
                    newly_installed: values[2].parse()?,
                    removed: values[5].parse()?,
                    not_upgraded: values[9].parse()?,
                }));
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "did not find proposed apt changes in output").into())
    }
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

    #[test]
    fn preserves_inline_deb822_signing_key_as_one_option() {
        let output = [
            "##PYINFRA_FILE /etc/apt/sources.list.d/vendor.sources".to_owned(),
            "Types: deb".to_owned(),
            "URIs: https://vendor".to_owned(),
            "Suites: stable".to_owned(),
            "Signed-By:".to_owned(),
            " -----BEGIN PGP PUBLIC KEY BLOCK-----".to_owned(),
            " .".to_owned(),
            " abc123".to_owned(),
            " -----END PGP PUBLIC KEY BLOCK-----".to_owned(),
        ];
        let repositories = AptSources.process(&output, &()).unwrap();
        let key = &repositories[0].options["signed-by"];
        assert_eq!(key.len(), 1);
        assert!(key[0].contains("-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nabc123"));
    }
}

#[cfg(test)]
#[path = "apt/package_tests.rs"]
mod package_tests;
