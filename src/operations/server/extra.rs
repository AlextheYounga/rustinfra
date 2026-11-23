use std::error::Error;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::api::{Command, CommandBit, ContentUploadCommand, StringCommand, remote_temporary_filename};
use crate::connectors::Connector;
use crate::facts::files::{FileContents, PathInput};
use crate::facts::server::{
    ETC_HOSTS_PATH, EtcHosts as EtcHostsFact, Kernel, KernelModules, Mounts, ServerKernel, Timezone as TimezoneFact,
    Which, is_bsd,
};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::files::{Put, PutInput, Template, TemplateInput};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModprobeInput {
    pub modules: Vec<String>,
    pub present: bool,
    pub force: bool,
}
impl ModprobeInput {
    #[must_use]
    pub fn new(module: impl Into<String>) -> Self {
        Self { modules: vec![module.into()], present: true, force: false }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Modprobe;
impl StatefulOperation for Modprobe {
    type Input = ModprobeInput;
    fn name(&self) -> &'static str {
        "server.modprobe"
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
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            let kernel = kernel(&facts).await?;
            let loaded = facts.get(&KernelModules, &kernel).await?;
            let changed =
                input.modules.iter().filter(|module| loaded.contains_key(*module) != input.present).collect::<Vec<_>>();
            if changed.is_empty() {
                return Ok(Vec::new());
            }
            let mut bits = vec![raw("modprobe")];
            if input.force {
                bits.push(raw("-f"));
            }
            if !input.present {
                bits.push(raw("-r"));
            }
            bits.push(raw("-a"));
            bits.extend(changed.into_iter().map(|value| quoted(value)));
            Ok(vec![shell(bits)])
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountInput {
    pub path: String,
    pub mounted: bool,
    pub options: Vec<String>,
    pub device: Option<String>,
    pub filesystem_type: Option<String>,
}
impl MountInput {
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into(), mounted: true, options: Vec::new(), device: None, filesystem_type: None }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Mount;
impl StatefulOperation for Mount {
    type Input = MountInput;
    fn name(&self) -> &'static str {
        "server.mount"
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
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            let kernel_name = facts.get(&Kernel, &()).await?.unwrap_or_default();
            let kernel = server_kernel(&kernel_name);
            let mounts = facts.get(&Mounts, &kernel).await?;
            let mounted_path = if mounts.contains_key(&input.path) {
                input.path.as_str()
            } else {
                input
                    .device
                    .as_ref()
                    .and_then(|device| {
                        mounts.iter().find(|(_, info)| &info.device == device).map(|(path, _)| path.as_str())
                    })
                    .unwrap_or(&input.path)
            };
            let current = mounts.get(mounted_path);
            if input.mounted && current.is_none() {
                let mut bits = vec![raw("mount")];
                optional(&mut bits, "-t", input.filesystem_type.as_deref());
                if !input.options.is_empty() {
                    bits.extend([raw("-o"), quoted(input.options.join(","))]);
                }
                if let Some(device) = &input.device {
                    bits.push(quoted(device));
                }
                bits.push(quoted(&input.path));
                return Ok(vec![shell(bits)]);
            }
            if !input.mounted && current.is_some() {
                return Ok(vec![shell([raw("umount"), quoted(mounted_path)])]);
            }
            if input.mounted
                && current.is_some_and(|info| input.options.iter().any(|value| !info.options.contains(value)))
            {
                let joined = input.options.join(",");
                return Ok(vec![if is_bsd(Some(&kernel_name)) {
                    let info = current.ok_or("missing mount")?;
                    shell([
                        raw("mount"),
                        raw("-uo"),
                        quoted(joined),
                        raw("-t"),
                        quoted(&info.filesystem_type),
                        quoted(&info.device),
                        quoted(mounted_path),
                    ])
                } else {
                    shell([raw("mount"), raw("-o"), quoted(format!("remount,{joined}")), quoted(mounted_path)])
                }]);
            }
            Ok(Vec::new())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EtcHostsInput {
    pub ip: String,
    pub hostnames: Vec<String>,
    pub present: bool,
    pub path: String,
}
impl EtcHostsInput {
    #[must_use]
    pub fn new(ip: impl Into<String>, hostnames: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            ip: ip.into(),
            hostnames: hostnames.into_iter().map(Into::into).collect(),
            present: true,
            path: ETC_HOSTS_PATH.to_owned(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct EtcHosts;
impl StatefulOperation for EtcHosts {
    type Input = EtcHostsInput;
    fn name(&self) -> &'static str {
        "server.etc_hosts"
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
        Box::pin(async move {
            if input.present && input.hostnames.is_empty() {
                return Err("hostnames must be provided when present=true".into());
            }
            let facts = FactExecutor::new(connector, options);
            let entries = facts.get(&EtcHostsFact, &Some(input.path.clone())).await?;
            let current = entries.get(&input.ip);
            if input.present && current == Some(&input.hostnames) || !input.present && current.is_none() {
                return Ok(Vec::new());
            }
            if !input.present
                && !input.hostnames.is_empty()
                && !current.is_some_and(|values| values.iter().any(|value| input.hostnames.contains(value)))
            {
                return Ok(Vec::new());
            }
            let existing = facts.get(&FileContents, &PathInput::from(input.path.clone())).await?.unwrap_or_default();
            let content = rewrite_hosts(input, &existing);
            Ok(vec![Command::UploadContent(ContentUploadCommand {
                content: content.into_bytes(),
                destination: input.path.clone(),
                remote_temporary_filename: None,
            })])
        })
    }
}
fn rewrite_hosts(input: &EtcHostsInput, lines: &[String]) -> String {
    let mut found = false;
    let mut output = Vec::new();
    for line in lines {
        let tokens = line.split('#').next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
        if tokens.first().copied() != Some(input.ip.as_str()) {
            output.push(line.trim_end_matches(['\r', '\n']).to_owned());
            continue;
        }
        found = true;
        if input.present {
            output.push(format!("{} {}", input.ip, input.hostnames.join(" ")));
        } else if !input.hostnames.is_empty() {
            let remaining = tokens[1..]
                .iter()
                .filter(|name| !input.hostnames.iter().any(|value| value == **name))
                .copied()
                .collect::<Vec<_>>();
            if !remaining.is_empty() {
                output.push(format!("{} {}", input.ip, remaining.join(" ")));
            }
        }
    }
    if input.present && !found {
        output.push(format!("{} {}", input.ip, input.hostnames.join(" ")));
    }
    if output.is_empty() { String::new() } else { format!("{}\n", output.join("\n")) }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptInput {
    pub source: PathBuf,
    pub arguments: Vec<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Script;
impl StatefulOperation for Script {
    type Input = ScriptInput;
    fn name(&self) -> &'static str {
        "server.script"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let destination = remote_temporary_filename(
                &options.run.remote_temp_directory,
                input.source.to_string_lossy().as_bytes(),
            );
            let put = PutInput::new(&input.source, &destination);
            let mut commands = Put.commands(&put, connector, options).await?;
            commands.push(shell([raw("chmod"), raw("+x"), quoted(&destination)]));
            let mut bits = vec![quoted(&destination)];
            bits.extend(input.arguments.iter().map(quoted));
            commands.push(shell(bits));
            Ok(commands)
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScriptTemplateInput {
    pub source: PathBuf,
    pub arguments: Vec<String>,
    pub data: Map<String, Value>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ScriptTemplate;
impl StatefulOperation for ScriptTemplate {
    type Input = ScriptTemplateInput;
    fn name(&self) -> &'static str {
        "server.script_template"
    }
    fn is_idempotent(&self) -> bool {
        false
    }
    fn commands<'a>(
        &'a self,
        input: &'a Self::Input,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let destination = remote_temporary_filename(
                &options.run.remote_temp_directory,
                format!("{}{:?}", input.source.display(), input.data).as_bytes(),
            );
            let mut template = TemplateInput::new(&input.source, &destination);
            template.data = input.data.clone();
            let mut commands = Template.commands(&template, connector, options).await?;
            commands.push(shell([raw("chmod"), raw("+x"), quoted(&destination)]));
            let mut bits = vec![quoted(&destination)];
            bits.extend(input.arguments.iter().map(quoted));
            commands.push(shell(bits));
            Ok(commands)
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timezone;
impl StatefulOperation for Timezone {
    type Input = String;
    fn name(&self) -> &'static str {
        "server.timezone"
    }
    fn is_idempotent(&self) -> bool {
        true
    }
    fn commands<'a>(
        &'a self,
        input: &'a String,
        connector: &'a dyn Connector,
        options: &'a FactOptions,
    ) -> StatefulOperationFuture<'a> {
        Box::pin(async move {
            let facts = FactExecutor::new(connector, options);
            if facts.get(&TimezoneFact, &()).await?.as_ref() == Some(input) {
                return Ok(Vec::new());
            }
            if facts.get(&Which, &"timedatectl".to_owned()).await?.is_some() {
                Ok(vec![shell([raw("timedatectl"), raw("set-timezone"), quoted(input)])])
            } else {
                Ok(vec![
                    shell([
                        raw("ln"),
                        raw("-sf"),
                        quoted(format!("/usr/share/zoneinfo/{input}")),
                        raw("/etc/localtime"),
                    ]),
                    shell([raw("echo"), quoted(input), raw("> /etc/timezone")]),
                ])
            }
        })
    }
}

async fn kernel(facts: &FactExecutor<'_>) -> Result<ServerKernel, Box<dyn Error + Send + Sync>> {
    Ok(server_kernel(facts.get(&Kernel, &()).await?.as_deref().unwrap_or_default()))
}
fn server_kernel(name: &str) -> ServerKernel {
    match name {
        "FreeBSD" => ServerKernel::FreeBsd,
        "OpenBSD" => ServerKernel::OpenBsd,
        "Darwin" => ServerKernel::Darwin,
        _ => ServerKernel::Linux,
    }
}
fn optional(bits: &mut Vec<CommandBit>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        bits.extend([raw(flag), quoted(value)]);
    }
}
fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<String>) -> CommandBit {
    CommandBit::quoted(value.into())
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosts_rewrite_preserves_unrelated_lines_and_removes_selected_names() {
        let mut input = EtcHostsInput::new("10.0.0.1", ["old"]);
        input.present = false;
        assert_eq!(
            rewrite_hosts(&input, &["# comment".to_owned(), "10.0.0.1 old keep # x".to_owned()]),
            "# comment\n10.0.0.1 keep\n"
        );
    }
}
