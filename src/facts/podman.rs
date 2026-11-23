use std::error::Error;
use std::io;

use serde_json::{Map, Value};

use crate::api::StringCommand;

use super::Fact;

fn parse_json(output: &[String]) -> Result<Value, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_str(&output.join(""))?)
}

fn expected_type(expected: &str, actual: &Value) -> Box<dyn Error + Send + Sync> {
    io::Error::new(io::ErrorKind::InvalidData, format!("expected podman {expected}, got {actual}")).into()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PodmanSystemInfo;

impl Fact for PodmanSystemInfo {
    type Input = ();
    type Output = Map<String, Value>;

    fn name(&self) -> &'static str {
        "podman.PodmanSystemInfo"
    }
    fn command(&self, _input: &Self::Input) -> StringCommand {
        StringCommand::new(["podman system info --format=json"])
    }
    fn default(&self) -> Self::Output {
        Map::new()
    }
    fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
        Some("podman")
    }
    fn process(&self, output: &[String], _input: &Self::Input) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
        let parsed = parse_json(output)?;
        match parsed {
            Value::Object(value) => Ok(value),
            value => Err(expected_type("object", &value)),
        }
    }
}

macro_rules! podman_list_fact {
    ($name:ident, $fact_name:literal, $command:literal) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl Fact for $name {
            type Input = ();
            type Output = Vec<Value>;

            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _input: &Self::Input) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                Vec::new()
            }
            fn requires_command(&self, _input: &Self::Input) -> Option<&str> {
                Some("podman")
            }
            fn process(
                &self,
                output: &[String],
                _input: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                let parsed = parse_json(output)?;
                match parsed {
                    Value::Array(value) => Ok(value),
                    value => Err(expected_type("array", &value)),
                }
            }
        }
    };
}

podman_list_fact!(PodmanPs, "podman.PodmanPs", "podman ps --format=json --all");
podman_list_fact!(PodmanImages, "podman.PodmanImages", "podman image ls --format=json --all");
podman_list_fact!(PodmanVolumes, "podman.PodmanVolumes", "podman volume ls --format=json");
podman_list_fact!(PodmanPods, "podman.PodmanPods", "podman pod ps --format=json");
podman_list_fact!(PodmanQuadlets, "podman.PodmanQuadlets", "podman quadlet list --format=json");
podman_list_fact!(PodmanNetworks, "podman.PodmanNetworks", "podman network ls --format=json");

#[cfg(test)]
mod tests {
    use super::{Fact, PodmanImages, PodmanPs, PodmanSystemInfo};

    #[test]
    fn parses_object_and_list_outputs_without_losing_json_types() {
        let info = PodmanSystemInfo.process(&[r#"{"host":{"cpus":6,"rootless":true}}"#.to_owned()], &()).unwrap();
        assert_eq!(info["host"]["cpus"], 6);
        assert_eq!(info["host"]["rootless"], true);

        let containers = PodmanPs.process(&[r#"[{"Names":["test"],"Pid":74195}]"#.to_owned()], &()).unwrap();
        assert_eq!(containers[0]["Names"][0], "test");
        assert_eq!(containers[0]["Pid"], 74_195);
        assert!(PodmanImages.process(&["[]".to_owned()], &()).unwrap().is_empty());
    }

    #[test]
    fn rejects_json_with_the_wrong_top_level_type() {
        assert!(PodmanSystemInfo.process(&["[]".to_owned()], &()).is_err());
        assert!(PodmanPs.process(&["{}".to_owned()], &()).is_err());
    }
}
