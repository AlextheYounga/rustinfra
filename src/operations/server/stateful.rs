use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{FileContents, PathInput};
use crate::facts::server::{Groups, Hostname as HostnameFact, Os, Which};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum HostnameFile {
    #[default]
    Auto,
    Disabled,
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostnameInput {
    pub hostname: String,
    pub file: HostnameFile,
}

impl HostnameInput {
    #[must_use]
    pub fn new(hostname: impl Into<String>) -> Self {
        Self { hostname: hostname.into(), file: HostnameFile::Auto }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hostname;

impl StatefulOperation for Hostname {
    type Input = HostnameInput;

    fn name(&self) -> &'static str {
        "server.hostname"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, fact_options);
            let current = facts.get(&HostnameFact, &()).await?;
            let hostnamectl = facts.get(&Which, &"hostnamectl".to_owned()).await?;
            if hostnamectl.is_some() {
                return Ok(if current.as_deref() == Some(input.hostname.as_str()) {
                    Vec::new()
                } else {
                    vec![command([raw("hostnamectl"), raw("set-hostname"), quoted(&input.hostname)])]
                });
            }

            let mut commands = Vec::new();
            if current.as_deref() != Some(input.hostname.as_str()) {
                commands.push(command([raw("hostname"), quoted(&input.hostname)]));
            }
            let path = match &input.file {
                HostnameFile::Disabled => None,
                HostnameFile::Path(path) => Some(path.as_str()),
                HostnameFile::Auto => match facts.get(&Os, &()).await?.as_deref() {
                    Some("Linux") => Some("/etc/hostname"),
                    Some("OpenBSD") => Some("/etc/myname"),
                    _ => None,
                },
            };
            if let Some(path) = path {
                let contents = facts.get(&FileContents, &PathInput::from(path)).await?;
                if contents.as_deref() != Some(slice::from_ref(&input.hostname)) {
                    commands.push(command([
                        raw("printf"),
                        raw("'%s\\n'"),
                        quoted(&input.hostname),
                        raw(">"),
                        quoted(path),
                    ]));
                }
            }
            Ok(commands)
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupInput {
    pub group: String,
    pub present: bool,
    pub system: bool,
    pub gid: Option<u64>,
}

impl GroupInput {
    #[must_use]
    pub fn new(group: impl Into<String>) -> Self {
        Self { group: group.into(), present: true, system: false, gid: None }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Group;

impl StatefulOperation for Group {
    type Input = GroupInput;

    fn name(&self) -> &'static str {
        "server.group"
    }

    fn is_idempotent(&self) -> bool {
        true
    }

    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        fact_options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, fact_options);
            let groups = facts.get(&Groups, &()).await?;
            let os = facts.get(&Os, &()).await?;
            Ok(group_commands(input, &groups, os.as_deref()))
        })
    }
}

fn group_commands(input: &GroupInput, groups: &[String], os: Option<&str>) -> Vec<Command> {
    let present = groups.iter().any(|group| group == &input.group);
    let freebsd = os == Some("FreeBSD");
    if present && !input.present {
        return if freebsd {
            vec![command([raw("pw"), raw("groupdel"), raw("-n"), quoted(&input.group)])]
        } else {
            vec![command([raw("groupdel"), quoted(&input.group)])]
        };
    }
    if present || !input.present {
        return Vec::new();
    }

    let mut bits = if freebsd { vec![raw("pw"), raw("groupadd")] } else { vec![raw("groupadd")] };
    if input.system && !os.is_some_and(|os| os.contains("BSD")) {
        bits.push(raw("-r"));
    }
    if freebsd {
        bits.extend([raw("-n"), quoted(&input.group)]);
    } else {
        bits.push(quoted(&input.group));
    }
    if let Some(gid) = input.gid {
        bits.extend([raw(if freebsd { "-g" } else { "--gid" }), quoted(&gid.to_string())]);
    }
    vec![command(bits)]
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

fn command(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{GroupInput, Hostname, HostnameFile, HostnameInput, group_commands};
    use crate::api::Command;
    use crate::connectors::LocalConnector;
    use crate::facts::FactOptions;
    use crate::operations::StatefulOperation;

    #[tokio::test]
    async fn current_hostname_with_disabled_file_is_a_noop() {
        let current = Hostname
            .commands(
                &HostnameInput {
                    hostname: fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned(),
                    file: HostnameFile::Disabled,
                },
                &LocalConnector,
                &FactOptions::default(),
            )
            .await
            .unwrap();

        assert!(current.is_empty());
    }

    #[test]
    fn group_matches_linux_and_freebsd_reference_commands() {
        let mut input = GroupInput::new("app users");
        input.system = true;
        input.gid = Some(1200);
        let linux = group_commands(&input, &[], Some("Linux"));
        let freebsd = group_commands(&input, &[], Some("FreeBSD"));

        assert!(
            matches!(&linux[0], Command::Shell(command) if command.raw_value() == "groupadd -r 'app users' --gid 1200")
        );
        assert!(
            matches!(&freebsd[0], Command::Shell(command) if command.raw_value() == "pw groupadd -n 'app users' -g 1200")
        );

        input.present = false;
        assert!(
            matches!(&group_commands(&input, &["app users".to_owned()], Some("FreeBSD"))[0], Command::Shell(command) if command.raw_value() == "pw groupdel -n 'app users'")
        );
    }
}
use std::slice;
