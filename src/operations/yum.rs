use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::FactOptions;

use super::rpm_manager::{
    PackageManagerConfig, YUM_REPOSITORY_DIRECTORY, packages_commands, repo_commands, rpm_commands,
};
pub use super::rpm_manager::{PackagesInput, RepoInput, RpmInput};
use super::{Operation, StatefulOperation, StatefulOperationFuture};

const PACKAGES: PackageManagerConfig = PackageManagerConfig {
    command: "yum",
    install_prefix: &["install", "-y"],
    remove_prefix: &["remove", "-y"],
    update_command: "yum update -y",
    clean_command: "yum clean all",
    supports_no_best: true,
    expand_providers: true,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Key;
impl Operation for Key {
    type Input = String;
    fn name(&self) -> &'static str {
        "yum.key"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, source: &String) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([CommandBit::from("rpm --import"), CommandBit::quoted(source.clone())]))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;
impl Operation for Update {
    type Input = ();
    fn name(&self) -> &'static str {
        "yum.update"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, _: &()) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([PACKAGES.update_command]))]
    }
}

macro_rules! stateful {
    ($name:ident, $operation_name:literal, $input:ty, $body:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl StatefulOperation for $name {
            type Input = $input;
            fn name(&self) -> &'static str {
                $operation_name
            }
            fn is_idempotent(&self) -> bool {
                true
            }
            fn commands<'a>(
                &'a self,
                input: &'a Self::Input,
                connector: &'a dyn Connector,
                options: &'a FactOptions,
            ) -> StatefulOperationFuture<'a> {
                Box::pin(async move { $body(input, connector, options).await })
            }
        }
    };
}

async fn yum_repo(
    input: &RepoInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    repo_commands(
        input,
        connector,
        options,
        super::rpm_manager::RepoConfig { directory: YUM_REPOSITORY_DIRECTORY, repository_type: None },
    )
    .await
}
async fn yum_rpm(
    input: &RpmInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    rpm_commands(input, connector, options, "yum").await
}
async fn yum_packages(
    input: &PackagesInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    packages_commands(input, connector, options, PACKAGES).await
}

stateful!(Repo, "yum.repo", RepoInput, yum_repo);
stateful!(Rpm, "yum.rpm", RpmInput, yum_rpm);
stateful!(Packages, "yum.packages", PackagesInput, yum_packages);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_and_update_match_reference_commands_and_quote_sources() {
        assert!(
            matches!(&Key.commands(&"https://example/key; id".to_owned())[0], Command::Shell(command) if command.raw_value() == "rpm --import 'https://example/key; id'")
        );
        assert!(matches!(&Update.commands(&())[0], Command::Shell(command) if command.raw_value() == "yum update -y"));
    }
}
use std::error::Error;
