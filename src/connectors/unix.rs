use indexmap::IndexMap;

use crate::api::{CommandBit, Config, EnvValue, HiddenValue, StringCommand};

const SUDO_ASKPASS_ENVIRONMENT_VARIABLE: &str = "PYINFRA_SUDO_PASSWORD";
const SU_ASKPASS_ENVIRONMENT_VARIABLE: &str = "PYINFRA_SU_PASSWORD";

/// Connector arguments that alter the Unix command sent to a host.
#[derive(Clone, Debug, PartialEq)]
pub struct UnixCommandOptions {
    pub environment: IndexMap<String, EnvValue>,
    pub working_directory: Option<String>,
    pub shell_executable: Option<String>,
    pub su_user: Option<String>,
    pub use_su_login: bool,
    pub su_shell: Option<String>,
    pub preserve_su_env: bool,
    pub su_password: Option<HiddenValue>,
    pub su_askpass_path: Option<String>,
    pub sudo: bool,
    pub sudo_user: Option<String>,
    pub use_sudo_login: bool,
    pub sudo_password: Option<HiddenValue>,
    pub sudo_askpass_path: Option<String>,
    pub preserve_sudo_env: bool,
    pub sudo_set_home: bool,
    pub doas: bool,
    pub doas_user: Option<String>,
    pub dzdo: bool,
    pub dzdo_user: Option<String>,
}

impl Default for UnixCommandOptions {
    fn default() -> Self {
        Self {
            environment: IndexMap::new(),
            working_directory: None,
            shell_executable: Some("sh".to_owned()),
            su_user: None,
            use_su_login: false,
            su_shell: None,
            preserve_su_env: false,
            su_password: None,
            su_askpass_path: None,
            sudo: false,
            sudo_user: None,
            use_sudo_login: false,
            sudo_password: None,
            sudo_askpass_path: None,
            preserve_sudo_env: false,
            sudo_set_home: true,
            doas: false,
            doas_user: None,
            dzdo: false,
            dzdo_user: None,
        }
    }
}

impl UnixCommandOptions {
    /// Builds connector arguments from deploy configuration and a process environment snapshot.
    ///
    /// Only names listed in `config.inherit_env` are inherited. Explicit `config.env` values then
    /// replace inherited values without changing their insertion positions, matching Python dicts.
    #[must_use]
    pub fn from_config(config: &Config, process_environment: &IndexMap<String, String>) -> Self {
        let mut environment = IndexMap::new();
        for name in &config.inherit_env {
            if let Some(value) = process_environment.get(name) {
                environment.insert(name.clone(), EnvValue::from(value.clone()));
            }
        }
        for (name, value) in &config.env {
            environment.insert(name.clone(), value.clone());
        }

        Self {
            environment,
            shell_executable: Some(config.shell.clone()),
            su_user: config.su_user.clone(),
            use_su_login: config.use_su_login,
            su_shell: config.su_shell.clone(),
            preserve_su_env: config.preserve_su_env,
            su_password: config.su_password.clone(),
            sudo: config.sudo,
            sudo_user: config.sudo_user.clone(),
            use_sudo_login: config.use_sudo_login,
            sudo_password: config.sudo_password.clone(),
            preserve_sudo_env: config.preserve_sudo_env,
            sudo_set_home: config.sudo_set_home,
            doas: config.doas,
            doas_user: config.doas_user.clone(),
            dzdo: config.dzdo,
            dzdo_user: config.dzdo_user.clone(),
            ..Self::default()
        }
    }

    /// Applies operation-level environment values over deploy defaults.
    pub fn apply_environment(&mut self, environment: &IndexMap<String, EnvValue>) {
        for (name, value) in environment {
            self.environment.insert(name.clone(), value.clone());
        }
    }
}

/// Applies environment, directory, shell, and privilege-escalation wrappers to a command.
#[must_use]
pub fn make_unix_command(command: StringCommand, options: &UnixCommandOptions) -> StringCommand {
    let command = apply_environment(command, &options.environment);
    let command = apply_working_directory(command, options.working_directory.as_deref());
    let mut bits = Vec::new();

    append_doas(&mut bits, options);
    append_dzdo(&mut bits, options);
    append_sudo_askpass(&mut bits, options);
    append_sudo(&mut bits, options);
    append_su(&mut bits, command, options);

    StringCommand::new(bits)
}

fn apply_environment(command: StringCommand, environment: &IndexMap<String, EnvValue>) -> StringCommand {
    if environment.is_empty() {
        return command;
    }

    let mut bits = vec![CommandBit::from("export")];
    for (key, value) in environment {
        let value = match value {
            EnvValue::String(value) => CommandBit::from(value.clone()),
            EnvValue::Hidden(value) => CommandBit::from(value.clone()),
        };
        let assignment = StringCommand::with_separator(vec![CommandBit::from(key.clone()), value], "=");
        bits.push(CommandBit::quoted(assignment));
    }
    bits.extend([CommandBit::from("&&"), command.into()]);
    StringCommand::new(bits)
}

fn apply_working_directory(command: StringCommand, working_directory: Option<&str>) -> StringCommand {
    let Some(directory) = working_directory else {
        return command;
    };
    StringCommand::new(vec![
        CommandBit::from("cd"),
        CommandBit::quoted(directory),
        CommandBit::from("&&"),
        command.into(),
    ])
}

fn append_doas(bits: &mut Vec<CommandBit>, options: &UnixCommandOptions) {
    if !options.doas {
        return;
    }
    bits.extend(["doas".into(), "-n".into()]);
    if let Some(user) = &options.doas_user {
        bits.extend(["-u".into(), CommandBit::quoted(user.clone())]);
    }
}

fn append_dzdo(bits: &mut Vec<CommandBit>, options: &UnixCommandOptions) {
    if !options.dzdo {
        return;
    }
    bits.extend(["dzdo".into(), "-H".into(), "-n".into()]);
    if let Some(user) = &options.dzdo_user {
        bits.extend(["-u".into(), CommandBit::quoted(user.clone())]);
    }
}

fn append_sudo_askpass(bits: &mut Vec<CommandBit>, options: &UnixCommandOptions) {
    let (Some(password), Some(path)) = (&options.sudo_password, &options.sudo_askpass_path) else {
        return;
    };
    let askpass =
        StringCommand::with_separator(vec![CommandBit::from("SUDO_ASKPASS="), CommandBit::quoted(path.clone())], "");
    let password = StringCommand::with_separator(
        vec![CommandBit::from(SUDO_ASKPASS_ENVIRONMENT_VARIABLE), CommandBit::quoted(password.clone())],
        "=",
    );
    bits.extend(["env".into(), askpass.into(), password.into()]);
}

fn append_sudo(bits: &mut Vec<CommandBit>, options: &UnixCommandOptions) {
    if !options.sudo {
        return;
    }
    bits.push("sudo".into());
    if options.sudo_set_home {
        bits.push("-H".into());
    }
    if options.sudo_password.is_some() {
        bits.extend(["-A".into(), "-k".into()]);
    } else {
        bits.push("-n".into());
    }
    if options.use_sudo_login {
        bits.push("-i".into());
    }
    if options.preserve_sudo_env {
        bits.push("-E".into());
    }
    if let Some(user) = &options.sudo_user {
        bits.extend(["-u".into(), CommandBit::quoted(user.clone())]);
    }
}

fn append_su(bits: &mut Vec<CommandBit>, command: StringCommand, options: &UnixCommandOptions) {
    let Some(user) = &options.su_user else {
        append_shell(bits, command, options.shell_executable.as_deref());
        return;
    };

    append_su_askpass(bits, options);
    bits.push("su".into());
    if options.use_su_login {
        bits.push("-l".into());
    }
    if options.preserve_su_env {
        bits.push("-m".into());
    }
    if let Some(shell) = &options.su_shell {
        let resolved_shell = StringCommand::with_separator(
            vec![CommandBit::from("$(command -v "), CommandBit::quoted(shell.clone()), CommandBit::from(")")],
            "",
        );
        bits.extend(["-s".into(), resolved_shell.into()]);
    }
    bits.extend([CommandBit::quoted(user.clone()), "-c".into()]);

    let inner = match &options.shell_executable {
        Some(shell) => StringCommand::new(vec![
            CommandBit::from(shell.clone()),
            CommandBit::from("-c"),
            CommandBit::quoted(command),
        ]),
        None => StringCommand::new(vec![CommandBit::from(command)]),
    };
    bits.push(CommandBit::quoted(inner));
}

fn append_su_askpass(bits: &mut Vec<CommandBit>, options: &UnixCommandOptions) {
    let (Some(password), Some(path)) = (&options.su_password, &options.su_askpass_path) else {
        return;
    };
    let password = StringCommand::with_separator(
        vec![CommandBit::from(SU_ASKPASS_ENVIRONMENT_VARIABLE), CommandBit::quoted(password.clone())],
        "=",
    );
    bits.extend(["env".into(), password.into(), CommandBit::quoted(path.clone()), "|".into()]);
}

fn append_shell(bits: &mut Vec<CommandBit>, command: StringCommand, shell: Option<&str>) {
    if let Some(shell) = shell {
        bits.extend([shell.into(), "-c".into(), CommandBit::quoted(command)]);
    } else {
        bits.push(command.into());
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{UnixCommandOptions, make_unix_command};
    use crate::api::{Config, EnvValue, HiddenValue, StringCommand};

    fn command(value: &str, options: &UnixCommandOptions) -> StringCommand {
        make_unix_command(StringCommand::new([value]), options)
    }

    #[test]
    fn default_command_uses_sh_and_quotes_the_command() {
        assert_eq!(command("echo Šablony", &UnixCommandOptions::default()).raw_value(), "sh -c 'echo Šablony'");
    }

    #[test]
    fn config_environment_overrides_inherited_values_and_operation_values_override_config() {
        let mut config = Config::default();
        config.inherit_env = vec!["FIRST".to_owned(), "TOKEN".to_owned()];
        config.env.insert("TOKEN".to_owned(), HiddenValue::new("config-secret").into());
        let process_environment = IndexMap::from([
            ("FIRST".to_owned(), "inherited".to_owned()),
            ("TOKEN".to_owned(), "old".to_owned()),
            ("IGNORED".to_owned(), "ignored".to_owned()),
        ]);
        let mut options = UnixCommandOptions::from_config(&config, &process_environment);
        let operation_environment = IndexMap::from([("FIRST".to_owned(), EnvValue::from("operation"))]);

        options.apply_environment(&operation_environment);

        assert_eq!(options.environment.keys().map(String::as_str).collect::<Vec<_>>(), vec!["FIRST", "TOKEN"]);
        assert_eq!(options.environment["FIRST"], EnvValue::from("operation"));
        assert!(matches!(options.environment["TOKEN"], EnvValue::Hidden(_)));
    }

    #[test]
    fn escalation_users_are_quoted_against_injection() {
        let options = UnixCommandOptions {
            doas: true,
            doas_user: Some("root; rm -rf /".to_owned()),
            dzdo: true,
            dzdo_user: Some("root`id`".to_owned()),
            sudo: true,
            sudo_user: Some("root; touch /tmp/pwn".to_owned()),
            ..UnixCommandOptions::default()
        };

        assert_eq!(
            command("uptime", &options).raw_value(),
            "doas -n -u 'root; rm -rf /' dzdo -H -n -u 'root`id`' sudo -H -n -u 'root; touch /tmp/pwn' sh -c uptime",
        );
    }

    #[test]
    fn environment_preserves_order_quotes_values_and_masks_secrets() {
        let mut options = UnixCommandOptions::default();
        options.environment.insert("key".to_owned(), EnvValue::from("value"));
        options.environment.insert("TOKEN".to_owned(), HiddenValue::new("abc; id").into());

        let command = command("uptime", &options);

        assert_eq!(command.raw_value(), "sh -c 'export key=value '\"'\"'TOKEN=abc; id'\"'\"' && uptime'");
        assert_eq!(command.masked_value(), "sh -c 'export key=value '\"'\"'TOKEN=*MASKED*'\"'\"' && uptime'");
    }

    #[test]
    fn mixed_wrappers_match_pyinfra_order_and_nested_quoting() {
        let mut options = UnixCommandOptions {
            working_directory: Some("/opt/somedir".to_owned()),
            shell_executable: Some("bash".to_owned()),
            sudo: true,
            sudo_user: Some("root".to_owned()),
            preserve_sudo_env: true,
            su_user: Some("pyinfra".to_owned()),
            ..UnixCommandOptions::default()
        };
        options.environment.insert("key".to_owned(), "value".into());

        assert_eq!(
            command("echo hi", &options).raw_value(),
            "sudo -H -n -E -u root su pyinfra -c 'bash -c '\"'\"'cd /opt/somedir && export key=value && echo hi'\"'\"''",
        );
    }
}
