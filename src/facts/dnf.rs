use std::error::Error;

use indexmap::IndexMap;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{Repository, parse_repositories};

#[derive(Clone, Copy, Debug, Default)]
pub struct DnfRepositories;
impl Fact for DnfRepositories {
    type Input = ();
    type Output = Vec<Repository>;
    fn name(&self) -> &'static str {
        "dnf.DnfRepositories"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([
            "for f in /etc/dnf.conf /etc/dnf.repos.d/*.repo /etc/yum.repos.d/*.repo; do [ -f \"$f\" ] && { printf \"#pyinfra-filename: %s\\n\" \"$f\"; cat \"$f\"; }; done || true",
        ])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("dnf")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_repositories(output))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DnfEnabledModules;
impl Fact for DnfEnabledModules {
    type Input = ();
    type Output = IndexMap<String, String>;
    fn name(&self) -> &'static str {
        "dnf.DnfEnabledModules"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["dnf module list --enabled"])
    }
    fn default(&self) -> Self::Output {
        IndexMap::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("dnf")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter(|line| line.split_whitespace().any(|value| value == "[e]"))
            .filter_map(|line| {
                let mut values = line.split_whitespace();
                Some((values.next()?.to_owned(), values.next()?.to_owned()))
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DnfDisabledModules;
impl Fact for DnfDisabledModules {
    type Input = ();
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "dnf.DnfDisabledModules"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["dnf module list --disabled"])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("dnf")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut modules = output
            .iter()
            .filter(|line| line.split_whitespace().any(|value| value == "[x]"))
            .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
            .collect::<Vec<_>>();
        modules.sort();
        modules.dedup();
        Ok(modules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repositories_and_module_flags() {
        let repos = DnfRepositories
            .process(
                &[
                    "#pyinfra-filename: /etc/yum.repos.d/test.repo".to_owned(),
                    "[test]".to_owned(),
                    "name = Test Repository".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(repos[0]["filename"], "/etc/yum.repos.d/test.repo");
        assert_eq!(repos[0]["name"], "Test Repository");
        let lines = [
            "postgresql 15 [x] profile".to_owned(),
            "postgresql 16 [x] profile".to_owned(),
            "nodejs 20 [e] profile".to_owned(),
        ];
        assert_eq!(DnfDisabledModules.process(&lines, &()).unwrap(), ["postgresql"]);
        assert_eq!(DnfEnabledModules.process(&lines, &()).unwrap()["nodejs"], "20");
    }
}
