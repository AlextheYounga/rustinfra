use indexmap::IndexMap;
use serde_json::Value;

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::docker::DockerPlugin;
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{command, optional, quoted, raw};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginInput {
    pub plugin: String,
    pub alias: Option<String>,
    pub present: bool,
    pub enabled: bool,
    pub options: IndexMap<String, String>,
}

impl PluginInput {
    #[must_use]
    pub fn new(plugin: impl Into<String>) -> Self {
        Self { plugin: plugin.into(), alias: None, present: true, enabled: true, options: IndexMap::new() }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Plugin;

impl StatefulOperation for Plugin {
    type Input = PluginInput;

    fn name(&self) -> &'static str {
        "docker.plugin"
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
            let name = input.alias.as_deref().unwrap_or(&input.plugin);
            let value = FactExecutor::new(connector, options).get(&DockerPlugin, &name.to_owned()).await?;
            Ok(plugin_commands(input, &value))
        })
    }
}

fn plugin_commands(input: &PluginInput, value: &Value) -> Vec<Command> {
    let name = input.alias.as_deref().unwrap_or(&input.plugin);
    let Some(current) = value.as_array().and_then(|values| values.first()) else {
        return install_commands(input);
    };
    if !input.present {
        return command([raw("docker"), raw("plugin"), raw("rm"), raw("-f"), quoted(name)]);
    }

    let enabled = current.get("Enabled").and_then(Value::as_bool).unwrap_or(false);
    let current_options = current.pointer("/Settings/Env").and_then(Value::as_object);
    let options_changed = input.options.iter().any(|(key, expected)| {
        current_options.and_then(|options| options.get(key)).and_then(Value::as_str) != Some(expected)
    });
    let mut commands = Vec::new();
    if options_changed {
        if enabled {
            commands.extend(plugin_action("disable", name));
        }
        let mut bits = vec![raw("docker"), raw("plugin"), raw("set"), quoted(name)];
        bits.extend(input.options.iter().map(|(key, value)| quoted(format!("{key}={value}"))));
        commands.extend(command(bits));
        if input.enabled {
            commands.extend(plugin_action("enable", name));
        }
    } else if enabled != input.enabled {
        commands.extend(plugin_action(if input.enabled { "enable" } else { "disable" }, name));
    }
    commands
}

fn install_commands(input: &PluginInput) -> Vec<Command> {
    if !input.present {
        return Vec::new();
    }
    let mut bits =
        vec![raw("docker"), raw("plugin"), raw("install"), quoted(&input.plugin), raw("--grant-all-permissions")];
    optional(&mut bits, "--alias", input.alias.as_deref());
    if !input.enabled {
        bits.push(raw("--disable"));
    }
    bits.extend(input.options.iter().map(|(key, value)| quoted(format!("{key}={value}"))));
    command(bits)
}

fn plugin_action(action: &str, name: &str) -> Vec<Command> {
    command([raw("docker"), raw("plugin"), raw(action), quoted(name)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptions(commands: &[Command]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                Command::Shell(command) => Some(command.raw_value()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn plugin_commands_match_install_remove_enable_and_option_fixtures() {
        let mut input = PluginInput::new("username/my-awesome-plugin:latest");
        assert_eq!(
            descriptions(&plugin_commands(&input, &Value::Array(Vec::new()))),
            ["docker plugin install username/my-awesome-plugin:latest --grant-all-permissions"]
        );
        input.enabled = false;
        assert_eq!(
            descriptions(&plugin_commands(&input, &Value::Array(Vec::new()))),
            ["docker plugin install username/my-awesome-plugin:latest --grant-all-permissions --disable"]
        );
        input.enabled = true;
        input.alias = Some("my-plugin".to_owned());
        input.options.insert("MY_VAR1".to_owned(), "value1".to_owned());
        input.options.insert("MY_VAR2".to_owned(), "value2".to_owned());
        assert_eq!(
            descriptions(&plugin_commands(&input, &Value::Array(Vec::new()))),
            [
                "docker plugin install username/my-awesome-plugin:latest --grant-all-permissions --alias my-plugin MY_VAR1=value1 MY_VAR2=value2"
            ]
        );

        input.present = false;
        let enabled = serde_json::json!([{"Enabled": true}]);
        assert_eq!(descriptions(&plugin_commands(&input, &enabled)), ["docker plugin rm -f my-plugin"]);
        let mut state_input = PluginInput::new("my-plugin");
        state_input.enabled = false;
        assert_eq!(descriptions(&plugin_commands(&state_input, &enabled)), ["docker plugin disable my-plugin"]);
        state_input.enabled = true;
        let disabled = serde_json::json!([{"Enabled": false}]);
        assert_eq!(descriptions(&plugin_commands(&state_input, &disabled)), ["docker plugin enable my-plugin"]);
    }

    #[test]
    fn plugin_option_changes_disable_set_and_restore_requested_state() {
        let mut input = PluginInput::new("my-plugin");
        input.options.insert("MY_VAR1".to_owned(), "value1".to_owned());
        input.options.insert("MY_VAR2".to_owned(), "value3".to_owned());
        let enabled = serde_json::json!([{
            "Enabled": true,
            "Settings": {"Env": {"MY_VAR1": "value1", "MY_VAR2": "value2"}}
        }]);
        assert_eq!(
            descriptions(&plugin_commands(&input, &enabled)),
            [
                "docker plugin disable my-plugin",
                "docker plugin set my-plugin MY_VAR1=value1 MY_VAR2=value3",
                "docker plugin enable my-plugin"
            ]
        );

        input.enabled = false;
        let disabled = serde_json::json!([{
            "Enabled": false,
            "Settings": {"Env": {"MY_VAR1": "value1", "MY_VAR2": "value2"}}
        }]);
        assert_eq!(
            descriptions(&plugin_commands(&input, &disabled)),
            ["docker plugin set my-plugin MY_VAR1=value1 MY_VAR2=value3"]
        );
    }
}
