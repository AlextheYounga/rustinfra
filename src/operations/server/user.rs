use std::error::Error;

use thiserror::Error;

use crate::api::{Command, CommandBit, HiddenValue, StringCommand};
use crate::connectors::Connector;
use crate::facts::files::{Directory as DirectoryFact, PathInfo, PathInput};
use crate::facts::server::{Groups, Os, UserInfo, Users, is_bsd};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserInput {
    pub user: String,
    pub present: bool,
    pub home: Option<String>,
    pub shell: Option<String>,
    pub group: Option<String>,
    pub groups: Vec<String>,
    pub append: bool,
    pub ensure_home: bool,
    pub create_home: bool,
    pub system: bool,
    pub uid: Option<u64>,
    pub comment: Option<String>,
    pub unique: bool,
    pub password: Option<HiddenValue>,
}

impl UserInput {
    #[must_use]
    pub fn new(user: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            present: true,
            home: None,
            shell: None,
            group: None,
            groups: Vec::new(),
            append: false,
            ensure_home: true,
            create_home: false,
            system: false,
            uid: None,
            comment: None,
            unique: true,
            password: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct User;

impl StatefulOperation for User {
    type Input = UserInput;

    fn name(&self) -> &'static str {
        "server.user"
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
            let users = facts.get(&Users, &()).await?;
            let groups = facts.get(&Groups, &()).await?;
            let os = facts.get(&Os, &()).await?;
            let existing = users.get(&input.user);
            let home = input
                .home
                .clone()
                .or_else(|| existing.and_then(|user| user.home.clone()))
                .unwrap_or_else(|| format!("/home/{}", input.user));
            let home_info = if input.present && input.ensure_home {
                Some(facts.get(&DirectoryFact, &PathInput::from(home.clone())).await?)
            } else {
                None
            };
            user_commands(
                input,
                &UserState { existing, existing_groups: &groups, os: os.as_deref(), home: &home, home_info },
            )
            .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
        })
    }
}

struct UserState<'a> {
    existing: Option<&'a UserInfo>,
    existing_groups: &'a [String],
    os: Option<&'a str>,
    home: &'a str,
    home_info: Option<PathInfo>,
}

fn user_commands(input: &UserInput, state: &UserState<'_>) -> Result<Vec<Command>, UserOperationError> {
    if !input.present {
        return Ok(state.existing.map_or_else(Vec::new, |_| vec![delete_user(&input.user, state.os)]));
    }

    let primary_group = input
        .group
        .as_deref()
        .or_else(|| state.existing_groups.iter().any(|group| group == &input.user).then_some(input.user.as_str()));
    let mut commands = if let Some(existing) = state.existing {
        modify_user(input, existing, state.os, state.home)
    } else {
        vec![create_user(input, primary_group, state.os, state.home)]
    };
    if input.ensure_home {
        add_home_commands(&mut commands, input, primary_group, state)?;
    }
    Ok(commands)
}

fn create_user(input: &UserInput, primary_group: Option<&str>, os: Option<&str>, home: &str) -> Command {
    let freebsd = os == Some("FreeBSD");
    let mut args = vec![raw("-d"), quoted(home)];
    if let Some(shell) = &input.shell {
        args.extend([raw("-s"), quoted(shell)]);
    }
    if let Some(group) = primary_group {
        args.extend([raw("-g"), quoted(group)]);
    }
    if !input.groups.is_empty() {
        args.extend([raw("-G"), group_list(&input.groups)]);
    }
    if input.system && !is_bsd(os) {
        args.push(raw("-r"));
    }
    if let Some(uid) = input.uid {
        args.extend([raw(if freebsd { "-u" } else { "--uid" }), quoted(&uid.to_string())]);
    }
    if let Some(comment) = &input.comment {
        args.extend([raw("-c"), quoted(comment)]);
    }
    if !input.unique {
        args.push(raw("-o"));
    }
    if input.create_home {
        args.push(raw("-m"));
    } else if !matches!(os, Some("FreeBSD" | "OpenBSD")) {
        args.push(raw("-M"));
    }
    if !freebsd && let Some(password) = &input.password {
        args.extend([raw("-p"), CommandBit::quoted(password.clone())]);
    }

    if freebsd {
        let mut bits = Vec::new();
        if let Some(password) = &input.password {
            bits.extend([raw("echo"), CommandBit::quoted(password.clone()), raw("|")]);
        }
        bits.extend([raw("pw"), raw("useradd"), raw("-n"), quoted(&input.user)]);
        if input.password.is_some() {
            bits.extend([raw("-H"), raw("0")]);
        }
        bits.extend(args);
        shell(bits)
    } else {
        let mut bits = vec![raw("useradd")];
        bits.extend(args);
        bits.push(quoted(&input.user));
        shell(bits)
    }
}

fn modify_user(input: &UserInput, existing: &UserInfo, os: Option<&str>, home: &str) -> Vec<Command> {
    let freebsd = os == Some("FreeBSD");
    let mut args = Vec::new();
    if existing.home.as_deref() != Some(home) {
        args.extend([raw("-d"), quoted(home)]);
    }
    if let Some(shell) = &input.shell
        && existing.shell.as_deref() != Some(shell)
    {
        args.extend([raw("-s"), quoted(shell)]);
    }
    if let Some(group) = &input.group
        && existing.group != *group
    {
        args.extend([raw("-g"), quoted(group)]);
    }
    if !input.groups.is_empty() {
        let contains_all = input.groups.iter().all(|group| existing.groups.contains(group));
        let same_groups = contains_all && input.groups.len() == existing.groups.len();
        if (input.append && !contains_all) || (!input.append && !same_groups) {
            if input.append {
                args.push(raw("-a"));
            }
            args.extend([raw("-G"), group_list(&input.groups)]);
        }
    }
    if let Some(comment) = &input.comment
        && existing.comment.as_deref() != Some(comment)
    {
        args.extend([raw("-c"), quoted(comment)]);
    }
    if !freebsd
        && let Some(password) = &input.password
        && existing.password != password.unmask()
    {
        args.extend([raw("-p"), CommandBit::quoted(password.clone())]);
    }
    if args.is_empty() {
        return Vec::new();
    }
    let mut bits =
        if freebsd { vec![raw("pw"), raw("usermod"), raw("-n"), quoted(&input.user)] } else { vec![raw("usermod")] };
    bits.extend(args);
    if !freebsd {
        bits.push(quoted(&input.user));
    }
    vec![shell(bits)]
}

fn add_home_commands(
    commands: &mut Vec<Command>,
    input: &UserInput,
    primary_group: Option<&str>,
    state: &UserState<'_>,
) -> Result<(), UserOperationError> {
    let group = primary_group.unwrap_or(&input.user);
    match state.home_info.clone().unwrap_or(PathInfo::Missing) {
        PathInfo::Missing => {
            commands.push(shell([raw("mkdir"), raw("-p"), quoted(state.home)]));
            commands.push(shell([raw("chown"), quoted(&format!("{}:{group}", input.user)), quoted(state.home)]));
        }
        PathInfo::Present(info) if info.user != input.user || info.group != group => {
            commands.push(shell([raw("chown"), quoted(&format!("{}:{group}", input.user)), quoted(state.home)]));
        }
        PathInfo::Present(_) => {}
        PathInfo::DifferentType => {
            return Err(UserOperationError::HomeNotDirectory { path: state.home.to_owned() });
        }
    }
    Ok(())
}

fn delete_user(user: &str, os: Option<&str>) -> Command {
    if os == Some("FreeBSD") {
        shell([raw("pw"), raw("userdel"), raw("-n"), quoted(user)])
    } else {
        shell([raw("userdel"), quoted(user)])
    }
}

fn group_list(groups: &[String]) -> CommandBit {
    let mut bits = Vec::new();
    for group in groups {
        if !bits.is_empty() {
            bits.push(raw(","));
        }
        bits.push(quoted(group));
    }
    StringCommand::with_separator(bits, "").into()
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}

fn quoted(value: &str) -> CommandBit {
    CommandBit::quoted(value.to_owned())
}

fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum UserOperationError {
    #[error("home path {path} exists and is not a directory")]
    HomeNotDirectory { path: String },
}

#[cfg(test)]
mod tests {
    use super::{UserInfo, UserInput, UserState, user_commands};
    use crate::api::{Command, HiddenValue};
    use crate::facts::files::PathInfo;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    fn existing_user() -> UserInfo {
        UserInfo {
            home: Some("/home/someuser".to_owned()),
            comment: Some("Full Name".to_owned()),
            shell: Some("/bin/bash".to_owned()),
            group: "nowt".to_owned(),
            groups: vec!["group1".to_owned(), "group2".to_owned()],
            uid: 1000,
            gid: 1000,
            lastlog: None,
            password: "$old$".to_owned(),
        }
    }

    #[test]
    fn creates_linux_user_with_reference_argument_order_and_home() {
        let mut input = UserInput::new("someuser");
        input.home = Some("homedir".to_owned());
        input.shell = Some("shellbin".to_owned());
        input.group = Some("mygroup".to_owned());
        input.groups = vec!["secondary_group".to_owned(), "another".to_owned()];
        input.uid = Some(1000);
        input.system = true;
        input.comment = Some("Full Name".to_owned());
        input.create_home = true;
        input.password = Some(HiddenValue::new("$somecryptedpassword$"));

        assert_eq!(
            descriptions(
                &user_commands(
                    &input,
                    &UserState {
                        existing: None,
                        existing_groups: &[],
                        os: Some("Linux"),
                        home: "homedir",
                        home_info: Some(PathInfo::Missing),
                    },
                )
                .unwrap()
            ),
            [
                "useradd -d homedir -s shellbin -g mygroup -G secondary_group,another -r --uid 1000 -c 'Full Name' -m -p '$somecryptedpassword$' someuser",
                "mkdir -p homedir",
                "chown someuser:mygroup homedir"
            ]
        );
    }

    #[test]
    fn modifies_only_changed_user_fields_and_can_remove_user() {
        let mut input = UserInput::new("someuser");
        input.home = Some("/home/someotheruser".to_owned());
        input.shell = Some("/bin/false".to_owned());
        input.group = Some("somegroup".to_owned());
        input.groups = vec!["group1".to_owned(), "group2".to_owned(), "group3".to_owned()];
        input.comment = Some("New Full Name".to_owned());
        input.ensure_home = false;
        let existing = existing_user();
        assert_eq!(
            descriptions(
                &user_commands(
                    &input,
                    &UserState {
                        existing: Some(&existing),
                        existing_groups: &[],
                        os: Some("Linux"),
                        home: "/home/someotheruser",
                        home_info: None,
                    },
                )
                .unwrap()
            ),
            [
                "usermod -d /home/someotheruser -s /bin/false -g somegroup -G group1,group2,group3 -c 'New Full Name' someuser"
            ]
        );

        input.present = false;
        assert_eq!(
            descriptions(
                &user_commands(
                    &input,
                    &UserState {
                        existing: Some(&existing),
                        existing_groups: &[],
                        os: Some("Linux"),
                        home: "/home/someuser",
                        home_info: None,
                    },
                )
                .unwrap()
            ),
            ["userdel someuser"]
        );
    }
}
