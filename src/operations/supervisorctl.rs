use crate::api::{Command, CommandBit, StringCommand};
use crate::connectors::Connector;
use crate::facts::supervisorctl::{SupervisorctlOptions, SupervisorctlStatus, SupervisorctlStatusMap, command};
use crate::facts::{FactExecutor, FactOptions};

use super::{Operation, StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessInput {
    pub process: String,
    pub running: Option<bool>,
    pub restarted: bool,
    pub command: Option<String>,
    pub options: SupervisorctlOptions,
}

impl ProcessInput {
    #[must_use]
    pub fn new(process: impl Into<String>) -> Self {
        Self {
            process: process.into(),
            running: Some(true),
            restarted: false,
            command: None,
            options: SupervisorctlOptions::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Process;

impl StatefulOperation for Process {
    type Input = ProcessInput;

    fn name(&self) -> &'static str {
        "supervisorctl.process"
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
            let statuses = FactExecutor::new(connector, fact_options).get(&SupervisorctlStatus, &input.options).await?;
            Ok(process_commands(input, &statuses))
        })
    }
}

fn process_commands(input: &ProcessInput, statuses: &SupervisorctlStatusMap) -> Vec<Command> {
    let members = if input.process == "all" {
        statuses.keys().collect::<Vec<_>>()
    } else if let Some(group) = input.process.strip_suffix('*') {
        statuses.keys().filter(|name| name.starts_with(group)).collect()
    } else {
        Vec::new()
    };
    let (running, fully_running) = if members.is_empty() && input.process != "all" && !input.process.ends_with(":*") {
        let running = statuses.get(&input.process) == Some(&Some(true));
        (running, running)
    } else {
        (
            members.iter().any(|name| statuses.get(*name) == Some(&Some(true))),
            !members.is_empty() && members.iter().all(|name| statuses.get(*name) == Some(&Some(true))),
        )
    };

    let mut commands = Vec::new();
    if input.running == Some(false) && running {
        commands.push(action_command(&input.options, "stop", Some(&input.process)));
    }
    if input.running == Some(true) && !fully_running {
        commands.push(action_command(&input.options, "start", Some(&input.process)));
    }
    if input.restarted && running {
        commands.push(action_command(&input.options, "restart", Some(&input.process)));
    }
    if let Some(custom) = &input.command {
        commands.push(action_command(&input.options, custom, Some(&input.process)));
    }
    commands
}

fn action_command(options: &SupervisorctlOptions, action: &str, target: Option<&str>) -> Command {
    let mut bits = vec![command(options).into(), CommandBit::quoted(action)];
    if let Some(target) = target {
        bits.push(CommandBit::quoted(target));
    }
    Command::Shell(StringCommand::new(bits))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reread;

impl Operation for Reread {
    type Input = SupervisorctlOptions;
    fn name(&self) -> &'static str {
        "supervisorctl.reread"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![action_command(input, "reread", None)]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Reload;

impl Operation for Reload {
    type Input = SupervisorctlOptions;
    fn name(&self) -> &'static str {
        "supervisorctl.reload"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![action_command(input, "reload", None)]
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdateInput {
    pub groups: Option<Vec<String>>,
    pub options: SupervisorctlOptions,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Update;

impl Operation for Update {
    type Input = UpdateInput;
    fn name(&self) -> &'static str {
        "supervisorctl.update"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        let mut bits = vec![command(&input.options).into(), CommandBit::from("update")];
        if let Some(groups) = &input.groups {
            bits.extend(groups.iter().map(|group| CommandBit::quoted(group.as_str())));
        }
        vec![Command::Shell(StringCommand::new(bits))]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupInput {
    pub group: String,
    pub options: SupervisorctlOptions,
}

impl GroupInput {
    #[must_use]
    pub fn new(group: impl Into<String>) -> Self {
        Self { group: group.into(), options: SupervisorctlOptions::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Add;

impl Operation for Add {
    type Input = GroupInput;
    fn name(&self) -> &'static str {
        "supervisorctl.add"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![action_command(&input.options, "add", Some(&input.group))]
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Remove;

impl Operation for Remove {
    type Input = GroupInput;
    fn name(&self) -> &'static str {
        "supervisorctl.remove"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![action_command(&input.options, "remove", Some(&input.group))]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearInput {
    pub process: String,
    pub options: SupervisorctlOptions,
}

impl ClearInput {
    #[must_use]
    pub fn new(process: impl Into<String>) -> Self {
        Self { process: process.into(), options: SupervisorctlOptions::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Clear;

impl Operation for Clear {
    type Input = ClearInput;
    fn name(&self) -> &'static str {
        "supervisorctl.clear"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![action_command(&input.options, "clear", Some(&input.process))]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalInput {
    pub signal: String,
    pub process: String,
    pub options: SupervisorctlOptions,
}

impl SignalInput {
    #[must_use]
    pub fn new(signal: impl Into<String>, process: impl Into<String>) -> Self {
        Self { signal: signal.into(), process: process.into(), options: SupervisorctlOptions::default() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Signal;

impl Operation for Signal {
    type Input = SignalInput;
    fn name(&self) -> &'static str {
        "supervisorctl.signal"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands(&self, input: &Self::Input) -> Vec<Command> {
        vec![Command::Shell(StringCommand::new([
            command(&input.options).into(),
            CommandBit::from("signal"),
            CommandBit::quoted(input.signal.as_str()),
            CommandBit::quoted(input.process.as_str()),
        ]))]
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{ProcessInput, Signal, SignalInput, Update, UpdateInput, process_commands};
    use crate::api::Command;
    use crate::operations::Operation;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(value) => Some(value.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn process_handles_single_group_all_restart_and_custom_actions() {
        let statuses = IndexMap::from([
            ("web".to_owned(), Some(true)),
            ("workers:one".to_owned(), Some(true)),
            ("workers:two".to_owned(), Some(false)),
        ]);
        let group = ProcessInput::new("workers:*");
        assert_eq!(descriptions(&process_commands(&group, &statuses)), ["supervisorctl start 'workers:*'"]);
        let mut all = ProcessInput::new("all");
        all.running = Some(false);
        assert_eq!(descriptions(&process_commands(&all, &statuses)), ["supervisorctl stop all"]);
        let mut web = ProcessInput::new("web");
        web.running = None;
        web.restarted = true;
        web.command = Some("clear".to_owned());
        assert_eq!(
            descriptions(&process_commands(&web, &statuses)),
            ["supervisorctl restart web", "supervisorctl clear web"]
        );
    }

    #[test]
    fn update_and_signal_quote_each_user_value() {
        let update = UpdateInput {
            groups: Some(vec!["workers".to_owned(), "x; reboot".to_owned()]),
            options: Default::default(),
        };
        assert_eq!(descriptions(&Update.commands(&update)), ["supervisorctl update workers 'x; reboot'"]);
        assert_eq!(
            descriptions(&Signal.commands(&SignalInput::new("USR2", "workers:*"))),
            ["supervisorctl signal USR2 'workers:*'"]
        );
    }
}
