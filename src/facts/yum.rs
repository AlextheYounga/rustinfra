use std::error::Error;

use crate::api::StringCommand;

use super::Fact;
use super::packages::{Repository, parse_repositories};

#[derive(Clone, Copy, Debug, Default)]
pub struct YumRepositories;
impl Fact for YumRepositories {
    type Input = ();
    type Output = Vec<Repository>;
    fn name(&self) -> &'static str {
        "yum.YumRepositories"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([
            "for f in /etc/yum.conf /etc/yum.repos.d/*.repo; do [ -f \"$f\" ] && { printf \"#pyinfra-filename: %s\\n\" \"$f\"; cat \"$f\"; }; done || true",
        ])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("yum")
    }
    fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(parse_repositories(output))
    }
}
