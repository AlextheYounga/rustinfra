use std::error::Error;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::api::StringCommand;
use crate::facts::Fact;

use super::inspect::{containers_command, images_command, networks_command, volumes_command};
use super::{invalid, json};

fn objects(output: &[String]) -> Result<Vec<Map<String, Value>>, Box<dyn Error + Send + Sync>> {
    match json(output)? {
        Value::Array(values) => values
            .into_iter()
            .map(|value| value.as_object().cloned().ok_or_else(|| invalid("expected Docker object")))
            .collect(),
        _ => Err(invalid("expected Docker object array")),
    }
}

fn object_at(value: Option<&Value>, key: &str) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .and_then(|value| value.get(key))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn id(object: &Map<String, Value>, key: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid("Docker object has no string identifier"))
}

macro_rules! derived_fact {
    ($name:ident, $fact_name:literal, $output:ty, $command:expr, $process:expr) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = ();
            type Output = IndexMap<String, $output>;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, _: &()) -> StringCommand {
                StringCommand::new([$command])
            }
            fn default(&self) -> Self::Output {
                IndexMap::new()
            }
            fn requires_command(&self, _: &()) -> Option<&str> {
                Some("docker")
            }
            fn process(&self, output: &[String], _: &()) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                objects(output)?.into_iter().map($process).collect()
            }
        }
    };
}

derived_fact!(DockerContainerEnvs, "docker.DockerContainerEnvs", IndexMap<String, String>, containers_command(), |container: Map<String, Value>| {
    let key = id(&container, "Id")?;
    let mut env = IndexMap::new();
    if let Some(values) = container.get("Config").and_then(Value::as_object).and_then(|value| value.get("Env")).and_then(Value::as_array) {
        for entry in values.iter().filter_map(Value::as_str) {
            let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
            env.insert(name.to_owned(), value.to_owned());
        }
    }
    Ok((key, env))
});

derived_fact!(DockerContainerLabels, "docker.DockerContainerLabels", Map<String, Value>, containers_command(), |container: Map<String, Value>| {
    Ok((id(&container, "Id")?, object_at(container.get("Config"), "Labels")))
});

derived_fact!(
    DockerContainerMounts,
    "docker.DockerContainerMounts",
    Vec<Value>,
    containers_command(),
    |container: Map<String, Value>| {
        let value = container.get("Mounts").and_then(Value::as_array).cloned().unwrap_or_default();
        Ok((id(&container, "Id")?, value))
    }
);

derived_fact!(DockerContainerNetworks, "docker.DockerContainerNetworks", Map<String, Value>, containers_command(), |container: Map<String, Value>| {
    Ok((id(&container, "Id")?, object_at(container.get("NetworkSettings"), "Networks")))
});

derived_fact!(DockerContainerPorts, "docker.DockerContainerPorts", Map<String, Value>, containers_command(), |container: Map<String, Value>| {
    Ok((id(&container, "Id")?, object_at(container.get("NetworkSettings"), "Ports")))
});

derived_fact!(DockerImageLabels, "docker.DockerImageLabels", Map<String, Value>, images_command(), |image: Map<String, Value>| {
    let labels = object_at(image.get("Config"), "Labels");
    let labels = if labels.is_empty() { object_at(image.get("ContainerConfig"), "Labels") } else { labels };
    Ok((id(&image, "Id")?, labels))
});

derived_fact!(DockerImageLayers, "docker.DockerImageLayers", Vec<String>, images_command(), |image: Map<
    String,
    Value,
>| {
    let layers = image
        .get("RootFS")
        .and_then(Value::as_object)
        .and_then(|value| value.get("Layers"))
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    Ok((id(&image, "Id")?, layers))
});

derived_fact!(DockerNetworkLabels, "docker.DockerNetworkLabels", Map<String, Value>, networks_command(), |network: Map<String, Value>| {
    let labels = network.get("Labels").and_then(Value::as_object).cloned().unwrap_or_default();
    Ok((id(&network, "Id")?, labels))
});

derived_fact!(DockerVolumeLabels, "docker.DockerVolumeLabels", Map<String, Value>, volumes_command(), |volume: Map<String, Value>| {
    let labels = volume.get("Labels").and_then(Value::as_object).cloned().unwrap_or_default();
    Ok((id(&volume, "Name")?, labels))
});
