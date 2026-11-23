use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{Repository, parse_repositories};

#[derive(Clone, Copy, Debug, Default)]
pub struct ZypperRepositories;
impl Fact for ZypperRepositories {
    type Input = ();
    type Output = Vec<Repository>;
    fn name(&self) -> &'static str {
        "zypper.ZypperRepositories"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([
            "for f in /etc/zypp/repos.d/*.repo; do [ -f \"$f\" ] && { printf \"#pyinfra-filename: %s\\n\" \"$f\"; cat \"$f\"; }; done || true",
        ])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("zypper")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_repositories(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_parser_ignores_comments_and_defaults_name() {
        let repos = ZypperRepositories
            .process(
                &[
                    "#pyinfra-filename: /etc/zypp/repos.d/test.repo".to_owned(),
                    " # comment".to_owned(),
                    "[test]".to_owned(),
                    "baseurl=abc".to_owned(),
                ],
                &(),
            )
            .unwrap();
        assert_eq!(repos[0]["name"], "test");
        assert_eq!(repos[0]["baseurl"], "abc");
    }
}
