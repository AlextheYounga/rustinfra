use std::error::Error;

use indexmap::IndexMap;

use crate::api::{
    Command, CommandBit, ContentUploadCommand, StringCommand, URL_SCHEME_SEPARATOR, remote_temporary_filename, sha1_hex,
};
use crate::connectors::Connector;
use crate::facts::files::{Directory, File, PathInfo, PathInput, Sha1File};
use crate::facts::rpm::{InstalledPackages, RpmPackage, RpmPackageInfo, RpmPackageProvides, RpmPackages};
use crate::facts::{FactExecutor, FactOptions};

pub const YUM_REPOSITORY_DIRECTORY: &str = "/etc/yum.repos.d/";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoInput {
    pub source: String,
    pub present: bool,
    pub base_url: Option<String>,
    pub description: Option<String>,
    pub enabled: bool,
    pub gpg_check: bool,
    pub gpg_key: Option<String>,
}

impl RepoInput {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            present: true,
            base_url: None,
            description: None,
            enabled: true,
            gpg_check: true,
            gpg_key: None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct RepoConfig {
    pub directory: &'static str,
    pub repository_type: Option<&'static str>,
}

pub async fn repo_commands(
    input: &RepoInput,
    connector: &dyn Connector,
    fact_options: &FactOptions,
    config: RepoConfig,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let (name, url) = repository_name_and_url(&input.source);
    let filename = format!("{}{name}.repo", config.directory);
    let facts = FactExecutor::new(connector, fact_options);
    let current = facts.get(&File, &PathInput::from(filename.clone())).await?;
    if !input.present {
        return Ok(if matches!(current, PathInfo::Present(_)) {
            vec![shell([raw("rm"), raw("-f"), quoted(filename)])]
        } else {
            Vec::new()
        });
    }
    let directory = config.directory.trim_end_matches('/').to_owned();
    let directory_missing =
        !matches!(facts.get(&Directory, &PathInput::from(directory.clone())).await?, PathInfo::Present(_));
    if let Some(url) = url {
        if matches!(current, PathInfo::Present(_)) {
            return Ok(Vec::new());
        }
        let temporary = remote_temporary_filename(&fact_options.run.remote_temp_directory, url.as_bytes());
        let mut commands = Vec::new();
        if directory_missing {
            commands.push(shell([raw("mkdir"), raw("-p"), quoted(directory)]));
        }
        commands.extend([
            shell([raw("curl"), raw("-sSLf"), quoted(url), raw("-o"), quoted(temporary.clone())]),
            shell([raw("mv"), quoted(temporary), quoted(filename)]),
        ]);
        return Ok(commands);
    }
    let content = repo_content(input, &name, config.repository_type)?;
    let local_hash = sha1_hex(content.as_bytes());
    let remote_hash = facts.get(&Sha1File, &PathInput::from(filename.clone())).await?;
    if remote_hash.as_deref() == Some(&local_hash) {
        return Ok(Vec::new());
    }
    let mut commands = Vec::new();
    if directory_missing {
        commands.push(shell([raw("mkdir"), raw("-p"), quoted(directory)]));
    }
    commands.extend([
        Command::UploadContent(ContentUploadCommand {
            content: content.into_bytes(),
            destination: filename.clone(),
            remote_temporary_filename: None,
        }),
        shell([raw("chmod"), raw("644"), quoted(filename)]),
    ]);
    Ok(commands)
}

fn repository_name_and_url(source: &str) -> (String, Option<&str>) {
    if source.split_once(URL_SCHEME_SEPARATOR).is_some() {
        let filename = source.rsplit('/').next().unwrap_or(source);
        (filename.strip_suffix(".repo").unwrap_or(filename).to_owned(), Some(source))
    } else {
        (source.to_owned(), None)
    }
}

fn repo_content(
    input: &RepoInput,
    name: &str,
    repository_type: Option<&str>,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let base_url = input.base_url.as_deref().ok_or("base_url is required for named repositories")?;
    if let Some(keys) = &input.gpg_key {
        for key in keys.split_whitespace() {
            if !key.starts_with('/') && key.split_once(URL_SCHEME_SEPARATOR).is_none() {
                return Err(format!("gpg_key must be a URL or absolute path: {key}").into());
            }
        }
    }
    let mut lines = vec![
        format!("[{name}]"),
        format!("name={}", input.description.as_deref().unwrap_or(name)),
        format!("baseurl={base_url}"),
        format!("enabled={}", u8::from(input.enabled)),
        format!("gpgcheck={}", u8::from(input.gpg_check)),
    ];
    if let Some(repository_type) = repository_type {
        lines.push(format!("type={repository_type}"));
    }
    if let Some(key) = &input.gpg_key {
        lines.push(format!("gpgkey={key}"));
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RpmInput {
    pub source: String,
    pub present: bool,
}

impl RpmInput {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), present: true }
    }
}

pub async fn rpm_commands(
    input: &RpmInput,
    connector: &dyn Connector,
    fact_options: &FactOptions,
    remove_command: &str,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let facts = FactExecutor::new(connector, fact_options);
    let is_url = input.source.split_once(URL_SCHEME_SEPARATOR).is_some();
    let temporary = remote_temporary_filename(&fact_options.run.remote_temp_directory, input.source.as_bytes());
    let package_path = if is_url { format!("{temporary}.rpm") } else { input.source.clone() };
    let info = facts.get(&RpmPackage, &package_path).await?;
    let installed = if let Some(info) = &info { facts.get(&RpmPackage, &info.name).await? } else { None };
    Ok(rpm_plan(
        input,
        RpmPlan {
            is_url,
            temporary: &temporary,
            package_path: &package_path,
            info: &info,
            installed: &installed,
            remove_command,
        },
    ))
}

#[derive(Clone, Copy)]
struct RpmPlan<'a> {
    is_url: bool,
    temporary: &'a str,
    package_path: &'a str,
    info: &'a Option<RpmPackageInfo>,
    installed: &'a Option<RpmPackageInfo>,
    remove_command: &'a str,
}

fn rpm_plan(input: &RpmInput, plan: RpmPlan<'_>) -> Vec<Command> {
    let exists =
        matches!((plan.info, plan.installed), (Some(candidate), Some(current)) if candidate.version == current.version);
    let mut commands = Vec::new();
    if plan.is_url {
        commands.extend([
            shell([raw("curl"), raw("-sSLf"), quoted(input.source.clone()), raw("-o"), quoted(plan.temporary)]),
            shell([raw("mv"), quoted(plan.temporary), quoted(plan.package_path)]),
        ]);
    }
    if input.present && !exists {
        commands.push(if plan.info.is_some() {
            shell([raw("rpm"), raw("-i"), quoted(plan.package_path)])
        } else {
            let path = StringCommand::new([quoted(plan.package_path)]).raw_value();
            Command::Shell(StringCommand::new([format!("rpm -q `rpm -qp {path}` 2> /dev/null || rpm -i {path}")]))
        });
    } else if !input.present
        && exists
        && let Some(RpmPackageInfo { name, .. }) = plan.info
    {
        commands.push(shell([raw(plan.remove_command), raw("remove"), raw("-y"), quoted(name.as_str())]));
    }
    commands
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackagesInput {
    pub packages: Vec<String>,
    pub present: bool,
    pub latest: bool,
    pub update: bool,
    pub clean: bool,
    pub no_best: bool,
    pub extra_global_install_args: Vec<CommandBit>,
    pub extra_install_args: Vec<CommandBit>,
    pub extra_global_uninstall_args: Vec<CommandBit>,
    pub extra_uninstall_args: Vec<CommandBit>,
}

impl PackagesInput {
    pub fn new(packages: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            packages: packages.into_iter().map(Into::into).collect(),
            present: true,
            latest: false,
            update: false,
            clean: false,
            no_best: false,
            extra_global_install_args: Vec::new(),
            extra_install_args: Vec::new(),
            extra_global_uninstall_args: Vec::new(),
            extra_uninstall_args: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct PackageManagerConfig {
    pub command: &'static str,
    pub install_prefix: &'static [&'static str],
    pub remove_prefix: &'static [&'static str],
    pub update_command: &'static str,
    pub clean_command: &'static str,
    pub supports_no_best: bool,
    pub expand_providers: bool,
}

pub async fn packages_commands(
    input: &PackagesInput,
    connector: &dyn Connector,
    fact_options: &FactOptions,
    config: PackageManagerConfig,
) -> Result<Vec<Command>, Box<dyn Error + Send + Sync>> {
    let facts = FactExecutor::new(connector, fact_options);
    let installed = facts.get(&RpmPackages, &()).await?;
    let mut provided = IndexMap::new();
    if config.expand_providers {
        for package in &input.packages {
            let name = package.split_once('=').map_or(package.as_str(), |(name, _)| name);
            if installed.contains_key(name) {
                continue;
            }
            let providers = facts.get(&RpmPackageProvides, &name.to_owned()).await?;
            let available = providers
                .iter()
                .any(|(provider, version)| installed.get(provider).is_some_and(|versions| versions.contains(version)));
            provided.insert(name.to_owned(), available);
        }
    }
    Ok(package_commands(input, &installed, &provided, config))
}

fn package_commands(
    input: &PackagesInput,
    installed: &InstalledPackages,
    provided: &IndexMap<String, bool>,
    config: PackageManagerConfig,
) -> Vec<Command> {
    let mut commands = Vec::new();
    if input.clean {
        commands.push(Command::Shell(StringCommand::new([config.clean_command])));
    }
    if input.update {
        commands.push(Command::Shell(StringCommand::new([config.update_command])));
    }
    let changed = input
        .packages
        .iter()
        .filter(|package| package_needs_change(package, input, installed, provided))
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return commands;
    }
    let prefix = if input.present { config.install_prefix } else { config.remove_prefix };
    let globals = if input.present { &input.extra_global_install_args } else { &input.extra_global_uninstall_args };
    let extras = if input.present { &input.extra_install_args } else { &input.extra_uninstall_args };
    let mut bits = vec![raw(config.command)];
    bits.extend(globals.iter().cloned());
    bits.extend(prefix.iter().copied().map(raw));
    if input.present && input.no_best && config.supports_no_best {
        bits.push(raw("--nobest"));
    }
    bits.extend(extras.iter().cloned());
    bits.extend(changed.into_iter().map(|package| quoted((*package).clone())));
    commands.push(shell(bits));
    commands
}

fn package_needs_change(
    package: &str,
    input: &PackagesInput,
    installed: &InstalledPackages,
    provided: &IndexMap<String, bool>,
) -> bool {
    let (name, version) = package.split_once('=').map_or((package, None), |(name, version)| (name, Some(version)));
    let versions = installed.get(name);
    let provider_installed = provided.get(name).copied().unwrap_or(false);
    if !input.present {
        versions.is_some() || provider_installed
    } else if let Some(version) = version {
        versions.is_none_or(|values| !values.iter().any(|value| value == version))
    } else {
        versions.is_none() && !provider_installed || input.latest
    }
}

fn raw(value: impl Into<String>) -> CommandBit {
    CommandBit::from(value.into())
}
fn quoted(value: impl Into<CommandBit>) -> CommandBit {
    CommandBit::quoted(value)
}
fn shell(bits: impl IntoIterator<Item = CommandBit>) -> Command {
    Command::Shell(StringCommand::new(bits))
}

#[cfg(test)]
#[path = "rpm_manager_tests.rs"]
mod tests;
