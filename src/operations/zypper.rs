use crate::api::{Command, StringCommand};
use crate::connectors::Connector;
use crate::facts::FactOptions;

use super::rpm_manager::{PackageManagerConfig, packages_commands, repo_commands, rpm_commands};
pub use super::rpm_manager::{PackagesInput, RepoInput, RpmInput};
pub use super::yum::Key;
use super::{Operation, StatefulOperation, StatefulOperationFuture};

const PACKAGES: PackageManagerConfig = PackageManagerConfig {
    command: "zypper",
    install_prefix: &["--non-interactive", "--gpg-auto-import-keys", "install", "-y"],
    remove_prefix: &["--non-interactive", "remove", "-y"],
    update_command: "zypper --non-interactive --gpg-auto-import-keys update -y",
    clean_command: "zypper clean --all",
    supports_no_best: false,
    expand_providers: false,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;
impl Operation for Update {
    type Input = ();
    fn name(&self) -> &'static str {
        "zypper.update"
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

async fn zypper_repo(
    input: &RepoInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    repo_commands(
        input,
        connector,
        options,
        super::rpm_manager::RepoConfig { directory: "/etc/zypp/repos.d/", repository_type: Some("rpm-md") },
    )
    .await
}
async fn zypper_rpm(
    input: &RpmInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    rpm_commands(input, connector, options, "zypper --non-interactive").await
}
async fn zypper_packages(
    input: &PackagesInput,
    connector: &dyn Connector,
    options: &FactOptions,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    packages_commands(input, connector, options, PACKAGES).await
}

stateful!(Repo, "zypper.repo", RepoInput, zypper_repo);
stateful!(Rpm, "zypper.rpm", RpmInput, zypper_rpm);
stateful!(Packages, "zypper.packages", PackagesInput, zypper_packages);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_matches_reference_command() {
        assert!(
            matches!(&Update.commands(&())[0], Command::Shell(command) if command.raw_value() == "zypper --non-interactive --gpg-auto-import-keys update -y")
        );
    }
}
use std::error::Error;
