use std::error::Error;

use indexmap::IndexMap;
use serde_json::Value;

use crate::api::StringCommand;
use crate::facts::Fact;

use super::{json_lines, quote};

#[derive(Clone, Copy, Debug, Default)]
pub struct DockerVersion;
impl Fact for DockerVersion {
    type Input = ();
    type Output = String;
    fn name(&self) -> &'static str {
        "docker.DockerVersion"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new(["docker --version"])
    }
    fn default(&self) -> String {
        String::new()
    }
    fn requires_command(&self, _: &()) -> Option<&str> {
        Some("docker")
    }
    fn process(&self, output: &[String], _: &()) -> Result<String, Box<dyn Error + Send + Sync>> {
        Ok(output.join("").replace('\n', ""))
    }
}

macro_rules! json_lines_fact {
    ($name:ident, $fact_name:literal, $input:ty, $command:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = $input;
            type Output = Vec<Value>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, input: &Self::Input) -> StringCommand {
                StringCommand::new([$command(input)])
            }
            fn default(&self) -> Vec<Value> {
                Vec::new()
            }
            fn requires_command(&self, _: &Self::Input) -> Option<&str> {
                Some("docker")
            }
            fn process(&self, output: &[String], _: &Self::Input) -> Result<Vec<Value>, Box<dyn Error + Send + Sync>> {
                json_lines(output)
            }
        }
    };
}

json_lines_fact!(DockerContainerStats, "docker.DockerContainerStats", (), |_: &()| {
    "docker stats --no-stream --format '{{json .}}'".to_owned()
});
json_lines_fact!(DockerImageHistory, "docker.DockerImageHistory", String, |id: &String| format!(
    "docker image history --no-trunc --format '{{{{json .}}}}' {} 2>&- || true",
    quote(id)
));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DockerFsChange {
    pub change: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DockerContainerFsChanges;
impl Fact for DockerContainerFsChanges {
    type Input = String;
    type Output = Vec<DockerFsChange>;
    fn name(&self) -> &'static str {
        "docker.DockerContainerFsChanges"
    }
    fn command(&self, id: &String) -> StringCommand {
        StringCommand::new([format!("docker container diff {} 2>&- || true", quote(id))])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("docker")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        Ok(output
            .iter()
            .filter_map(|line| line.trim_end_matches('\n').split_once(' '))
            .map(|(change, path)| DockerFsChange { change: change.to_owned(), path: path.to_owned() })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DockerContainerProcesses;
impl Fact for DockerContainerProcesses {
    type Input = String;
    type Output = Vec<IndexMap<String, String>>;
    fn name(&self) -> &'static str {
        "docker.DockerContainerProcesses"
    }
    fn command(&self, id: &String) -> StringCommand {
        StringCommand::new([format!("docker container top {} 2>&- || true", quote(id))])
    }
    fn default(&self) -> Self::Output {
        Vec::new()
    }
    fn requires_command(&self, _: &String) -> Option<&str> {
        Some("docker")
    }
    fn process(&self, output: &[String], _: &String) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let mut lines = output.iter().map(|line| line.trim_end_matches('\n')).filter(|line| !line.is_empty());
        let Some(header) = lines.next() else {
            return Ok(Vec::new());
        };
        let columns = header.split_whitespace().collect::<Vec<_>>();
        Ok(lines
            .map(|line| {
                let mut values = line.splitn(columns.len(), char::is_whitespace).filter(|value| !value.is_empty());
                columns
                    .iter()
                    .map(|column| ((*column).to_owned(), values.next().unwrap_or_default().to_owned()))
                    .collect()
            })
            .collect())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DockerAuths;
impl Fact for DockerAuths {
    type Input = ();
    type Output = Vec<String>;
    fn name(&self) -> &'static str {
        "docker.DockerAuths"
    }
    fn command(&self, _: &()) -> StringCommand {
        StringCommand::new([
            "config=\"${DOCKER_CONFIG:-$HOME/.docker}/config.json\"; [ -r \"$config\" ] && cat \"$config\" || echo \"{}\"",
        ])
    }
    fn default(&self) -> Vec<String> {
        Vec::new()
    }
    fn process(&self, output: &[String], _: &()) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let Ok(value) = serde_json::from_str::<Value>(&output.join("")) else {
            return Ok(Vec::new());
        };
        Ok(value
            .get("auths")
            .and_then(Value::as_object)
            .map(|auths| auths.keys().cloned().collect())
            .unwrap_or_default())
    }
}
