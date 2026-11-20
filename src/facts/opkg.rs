use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{PackageMap, insert_package};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpkgConfInfo {
    pub paths: IndexMap<String, String>,
    pub list_dir: String,
    pub options: IndexMap<String, OpkgOption>,
    pub architectures: IndexMap<String, u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpkgOption {
    Enabled,
    Value(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpkgFeedInfo {
    pub url: String,
    pub format: String,
    pub kind: OpkgFeedKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpkgFeedKind {
    Distribution,
    Custom,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpkgPkgUpgradeInfo {
    pub installed: String,
    pub available: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpkgConf;

impl Fact for OpkgConf {
    type Input = ();
    type Output = OpkgConfInfo;

    fn name(&self) -> &'static str {
        "opkg.OpkgConf"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["cat /etc/opkg.conf"])
    }

    fn default(&self) -> Self::Output {
        OpkgConfInfo::default()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("opkg")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut config = OpkgConfInfo::default();
        for line in meaningful_lines(output) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            match fields.as_slice() {
                ["arch", architecture, priority] => {
                    if let Ok(priority) = priority.parse() {
                        config.architectures.insert((*architecture).to_owned(), priority);
                    }
                }
                ["dest", name, path] => {
                    config.paths.insert((*name).to_owned(), (*path).to_owned());
                }
                ["lists_dir", "ext", path] => config.list_dir = (*path).to_owned(),
                ["option", name] => {
                    config.options.insert((*name).to_owned(), OpkgOption::Enabled);
                }
                ["option", name, value] => {
                    config.options.insert((*name).to_owned(), OpkgOption::Value((*value).to_owned()));
                }
                _ => {}
            }
        }
        Ok(config)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpkgFeeds;

impl Fact for OpkgFeeds {
    type Input = ();
    type Output = IndexMap<String, OpkgFeedInfo>;

    fn name(&self) -> &'static str {
        "opkg.OpkgFeeds"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["cat /etc/opkg/distfeeds.conf; echo CUSTOM; cat /etc/opkg/customfeeds.conf"])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("opkg")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut feeds = IndexMap::new();
        let mut kind = OpkgFeedKind::Distribution;
        for line in meaningful_lines(output) {
            if line == "CUSTOM" {
                kind = OpkgFeedKind::Custom;
                continue;
            }
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if let [format, name, url] = fields.as_slice() {
                feeds.insert(
                    (*name).to_owned(),
                    OpkgFeedInfo { url: (*url).to_owned(), format: (*format).to_owned(), kind },
                );
            }
        }
        Ok(feeds)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpkgInstallableArchitectures;

impl Fact for OpkgInstallableArchitectures {
    type Input = ();
    type Output = IndexMap<String, u32>;

    fn name(&self) -> &'static str {
        "opkg.OpkgInstallableArchitectures"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["opkg print-architecture"])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("opkg")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut architectures = IndexMap::new();
        for line in meaningful_lines(output) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if let ["arch", architecture, priority] = fields.as_slice()
                && let Ok(priority) = priority.parse()
            {
                architectures.insert((*architecture).to_owned(), priority);
            }
        }
        Ok(architectures)
    }
}

macro_rules! package_fact {
    ($name:ident, $fact_name:literal, $command:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = ();
            type Output = PackageMap;

            fn name(&self) -> &'static str {
                $fact_name
            }

            fn command(&self, _input: &Self::Input) -> StringCommand {
                StringCommand::new([$command])
            }

            fn default(&self) -> Self::Output {
                PackageMap::new()
            }

            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some("opkg")
            }

            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                let mut packages = PackageMap::new();
                for line in output {
                    let fields = line.split(" - ").collect::<Vec<_>>();
                    if let [name, version] = fields.as_slice() {
                        insert_package(&mut packages, name, version);
                    }
                }
                Ok(packages)
            }
        }
    };
}

package_fact!(OpkgPackages, "opkg.OpkgPackages", "opkg list-installed");

#[derive(Clone, Copy, Debug, Default)]
pub struct OpkgUpgradeablePackages;

impl Fact for OpkgUpgradeablePackages {
    type Input = ();
    type Output = IndexMap<String, OpkgPkgUpgradeInfo>;

    fn name(&self) -> &'static str {
        "opkg.OpkgUpgradeablePackages"
    }

    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["opkg list-upgradable"])
    }

    fn default(&self) -> Self::Output {
        IndexMap::new()
    }

    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("opkg")
    }

    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut packages = IndexMap::new();
        for line in output {
            let fields = line.split(" - ").collect::<Vec<_>>();
            if let [name, installed, available] = fields.as_slice() {
                packages.insert(
                    (*name).to_owned(),
                    OpkgPkgUpgradeInfo { installed: (*installed).to_owned(), available: (*available).to_owned() },
                );
            }
        }
        Ok(packages)
    }
}

fn meaningful_lines(output: &[String]) -> impl Iterator<Item = &str> {
    output.iter().filter_map(|line| {
        let value = if let Some(value) = line.split('#').next() { value.trim() } else { "" };
        (!value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Fact, OpkgConf, OpkgFeedKind, OpkgFeeds, OpkgInstallableArchitectures, OpkgOption, OpkgPackages,
        OpkgUpgradeablePackages,
    };

    #[test]
    fn parses_configuration_and_architectures() {
        let config = OpkgConf
            .process(
                &[
                    "dest root /".to_owned(),
                    "lists_dir ext /var/opkg-lists".to_owned(),
                    "option check_signature".to_owned(),
                    "option overlay_root /overlay".to_owned(),
                    "arch brcm47xx 300 # comment".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(config.paths["root"], "/");
        assert_eq!(config.list_dir, "/var/opkg-lists");
        assert_eq!(config.options["check_signature"], OpkgOption::Enabled);
        assert_eq!(config.architectures["brcm47xx"], 300);

        let architectures =
            OpkgInstallableArchitectures.process(&["arch all 1".to_owned(), "arch invalid".to_owned()], &()).unwrap();
        assert_eq!(architectures["all"], 1);
    }

    #[test]
    fn parses_feeds_packages_and_upgrades() {
        let feeds = OpkgFeeds
            .process(
                &[
                    "src/gz openwrt_core http://downloads.openwrt.org/core".to_owned(),
                    "CUSTOM".to_owned(),
                    "src/gz custom http://example.org/packages".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(feeds["openwrt_core"].kind, OpkgFeedKind::Distribution);
        assert_eq!(feeds["custom"].kind, OpkgFeedKind::Custom);

        let packages = OpkgPackages.process(&["wget-nossl - 1.20.3-4".to_owned()], &()).unwrap();
        assert_eq!(packages["wget-nossl"], ["1.20.3-4"]);
        let upgrades = OpkgUpgradeablePackages.process(&["libuv1 - 1.34.2-1 - 1.40.0-1".to_owned()], &()).unwrap();
        assert_eq!(upgrades["libuv1"].available, "1.40.0-1");
    }
}
