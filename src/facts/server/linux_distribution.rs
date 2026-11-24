use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;
use crate::facts::Fact;

const COMMAND: &str = "cd /etc/ && for file in $(ls -pdL *-release | grep -v /); do echo \"/etc/${file}\"; cat \"/etc/${file}\"; echo ---; done";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LinuxDistributionInfo {
    pub name: Option<String>,
    pub major: Option<u32>,
    pub minor: Option<u32>,
    pub release_meta: IndexMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxDistribution;

impl Fact for LinuxDistribution {
    type Input = ();
    type Output = LinuxDistributionInfo;

    fn name(&self) -> &'static str {
        "server.LinuxDistribution"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new([COMMAND])
    }

    fn default(&self) -> Self::Output {
        LinuxDistributionInfo::default()
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let files = release_files(output);
        let mut metadata = IndexMap::new();
        for line in files.get("/etc/os-release").into_iter().flatten() {
            if let Some((key, value)) = line.split_once('=') {
                metadata.insert(key.to_ascii_uppercase(), value.trim_matches(['\'', '"']).to_owned());
            }
        }

        if !metadata.contains_key("CODENAME") {
            let codename = metadata.get("VERSION_CODENAME").cloned().or_else(|| {
                metadata.get("VERSION").and_then(|version| {
                    let start = version.find('(')? + 1;
                    let end = version[start..].find(')')? + start;
                    Some(version[start..end].to_owned())
                })
            });
            if let Some(codename) = codename {
                metadata.insert("CODENAME".to_owned(), codename);
            }
        }

        let fallback_release = files
            .iter()
            .filter(|(filename, _)| filename.as_str() != "/etc/os-release")
            .flat_map(|(_, lines)| lines)
            .find(|line| !line.trim().is_empty());
        let id = metadata.get("ID").map(String::as_str).unwrap_or_default();
        let name = match id {
            "alpine" => Some("Alpine"),
            "centos" => Some("CentOS"),
            "fedora" => Some("Fedora"),
            "gentoo" => Some("Gentoo"),
            "opensuse" => Some("openSUSE"),
            "rhel" => Some("RedHat"),
            "ubuntu" => Some("Ubuntu"),
            "debian" => Some("Debian"),
            _ => metadata.get("NAME").map(String::as_str).or_else(|| fallback_release.and_then(release_name)),
        }
        .map(str::to_owned);

        let metadata_version = metadata.get("VERSION_ID").and_then(|version| parse_version(version));
        let fallback_version = fallback_release.and_then(|line| line.split_whitespace().find_map(parse_version));
        let (major, minor) = match (metadata_version, fallback_version) {
            (Some((major, None)), Some((fallback_major, fallback_minor))) if major == fallback_major => {
                (Some(major), fallback_minor)
            }
            (Some((major, minor)), _) | (None, Some((major, minor))) => (Some(major), minor),
            (None, None) => (None, None),
        };

        Ok(LinuxDistributionInfo { name, major, minor, release_meta: metadata })
    }
}

fn release_files(output: &[String]) -> IndexMap<String, Vec<String>> {
    let mut files = IndexMap::new();
    let mut filename = None::<String>;
    for line in output {
        if line.starts_with("/etc/") {
            filename = Some(line.clone());
            files.entry(line.clone()).or_insert_with(Vec::new);
        } else if line == "---" {
            filename = None;
        } else if let Some(filename) = &filename {
            files.entry(filename.clone()).or_insert_with(Vec::new).push(line.clone());
        }
    }
    files
}

fn release_name(line: &String) -> Option<&str> {
    if line.starts_with("CentOS") {
        Some("CentOS")
    } else if line.starts_with("Fedora") {
        Some("Fedora")
    } else {
        None
    }
}

fn parse_version(value: &str) -> Option<(u32, Option<u32>)> {
    let value = value.trim_matches(|character: char| !character.is_ascii_digit() && character != '.');
    if !value.chars().next().is_some_and(|character| character.is_ascii_digit()) {
        return None;
    }
    let mut parts = value.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|part| part.parse().ok());
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::{Fact, LinuxDistribution};

    #[test]
    fn parses_os_release_name_version_and_codename() {
        let info = LinuxDistribution
            .process(
                &[
                    "/etc/os-release".to_owned(),
                    "NAME=Ubuntu".to_owned(),
                    "VERSION=\"24.04 LTS (Noble Numbat)\"".to_owned(),
                    "VERSION_ID=24.04".to_owned(),
                    "---".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(info.name.as_deref(), Some("Ubuntu"));
        assert_eq!(info.major, Some(24));
        assert_eq!(info.minor, Some(4));
        assert_eq!(info.release_meta["CODENAME"], "Noble Numbat");
    }

    #[test]
    fn uses_distribution_release_for_a_more_precise_centos_version() {
        let info = LinuxDistribution
            .process(
                &[
                    "/etc/centos-release".to_owned(),
                    "CentOS Linux release 8.4.2105".to_owned(),
                    "---".to_owned(),
                    "/etc/os-release".to_owned(),
                    "ID=centos".to_owned(),
                    "VERSION_ID=8".to_owned(),
                    "---".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(info.name.as_deref(), Some("CentOS"));
        assert_eq!(info.major, Some(8));
        assert_eq!(info.minor, Some(4));
    }

    #[test]
    fn parses_reference_distribution_variants_and_empty_output() {
        let cases = [
            (vec!["/etc/os-release", "NAME=\"Arch Linux\"", "ID=arch", "---"], Some("Arch Linux"), None, None),
            (
                vec![
                    "/etc/centos-release",
                    "CentOS release 6.9 (Final)",
                    "---",
                    "/etc/os-release",
                    "LSB_VERSION=base-4.0-amd64",
                    "---",
                ],
                Some("CentOS"),
                Some(6),
                Some(9),
            ),
            (
                vec![
                    "/etc/centos-release",
                    "CentOS Linux release 7.9.2009 (Core)",
                    "---",
                    "/etc/os-release",
                    "ID=centos",
                    "VERSION_ID=7",
                    "---",
                ],
                Some("CentOS"),
                Some(7),
                Some(9),
            ),
            (
                vec![
                    "/etc/centos-release",
                    "CentOS Linux release 8.4.2105",
                    "---",
                    "/etc/os-release",
                    "ID=centos",
                    "VERSION_ID=8",
                    "---",
                ],
                Some("CentOS"),
                Some(8),
                Some(4),
            ),
            (
                vec![
                    "/etc/centos-release",
                    "CentOS Stream release 9",
                    "---",
                    "/etc/os-release",
                    "ID=centos",
                    "VERSION_ID=9",
                    "---",
                ],
                Some("CentOS"),
                Some(9),
                None,
            ),
            (
                vec![
                    "/etc/fedora-release",
                    "Fedora release 40 (Forty)",
                    "---",
                    "/etc/os-release",
                    "ID=fedora",
                    "VERSION_ID=40",
                    "VERSION=\"40 (Forty)\"",
                    "---",
                ],
                Some("Fedora"),
                Some(40),
                None,
            ),
            (
                vec!["/etc/os-release", "NAME=\"RED OS\"", "ID=redos", "VERSION_ID=8.0", "---"],
                Some("RED OS"),
                Some(8),
                Some(0),
            ),
            (
                vec![
                    "/etc/os-release",
                    "NAME=Ubuntu",
                    "ID=ubuntu",
                    "VERSION=\"20.04 LTS (Focal Fossa)\"",
                    "VERSION_ID=20.04",
                    "---",
                ],
                Some("Ubuntu"),
                Some(20),
                Some(4),
            ),
        ];

        for (output, name, major, minor) in cases {
            let output = output.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let info = LinuxDistribution.process(&output, &()).unwrap();
            assert_eq!(info.name.as_deref(), name);
            assert_eq!(info.major, major);
            assert_eq!(info.minor, minor);
        }

        assert_eq!(LinuxDistribution.process(&[], &()).unwrap(), Default::default());
    }
}
