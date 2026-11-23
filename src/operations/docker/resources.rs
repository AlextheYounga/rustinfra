use std::collections::BTreeMap;

use serde_json::Value;

use crate::api::Command;
use crate::connectors::Connector;
use crate::facts::docker::{DockerContainer, DockerImage, DockerNetwork, DockerVolume};
use crate::facts::{FactExecutor, FactOptions};
use crate::operations::{StatefulOperation, StatefulOperationFuture};

use super::{command, json_exists, optional, pairs, quoted, raw, repeated};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageInput {
    pub image: String,
    pub present: bool,
    pub force: bool,
}
impl ImageInput {
    #[must_use]
    pub fn new(image: impl Into<String>) -> Self {
        Self { image: image.into(), present: true, force: false }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Image;
impl StatefulOperation for Image {
    type Input = ImageInput;
    fn name(&self) -> &'static str {
        "docker.image"
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
            let exists = json_exists(&FactExecutor::new(connector, options).get(&DockerImage, &input.image).await?);
            let latest = !input.image.contains('@')
                && (!input.image.rsplit('/').next().is_some_and(|name| name.contains(':'))
                    || input.image.ends_with(":latest"));
            if input.present && (input.force || latest || !exists) {
                Ok(command([raw("docker"), raw("image"), raw("pull"), quoted(&input.image)]))
            } else if !input.present && exists {
                Ok(command([raw("docker"), raw("image"), raw("rm"), quoted(&input.image)]))
            } else {
                Ok(Vec::new())
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VolumeInput {
    pub volume: String,
    pub driver: Option<String>,
    pub labels: Vec<String>,
    pub options: Vec<String>,
    pub present: bool,
}
impl VolumeInput {
    #[must_use]
    pub fn new(volume: impl Into<String>) -> Self {
        Self { volume: volume.into(), driver: None, labels: Vec::new(), options: Vec::new(), present: true }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Volume;
impl StatefulOperation for Volume {
    type Input = VolumeInput;
    fn name(&self) -> &'static str {
        "docker.volume"
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
            let exists = json_exists(&FactExecutor::new(connector, options).get(&DockerVolume, &input.volume).await?);
            if input.present && !exists {
                let mut bits = vec![raw("docker"), raw("volume"), raw("create"), quoted(&input.volume)];
                optional(&mut bits, "-d", input.driver.as_deref());
                repeated(&mut bits, "--label", &input.labels);
                repeated(&mut bits, "--opt", &input.options);
                Ok(command(bits))
            } else if !input.present && exists {
                Ok(command([raw("docker"), raw("volume"), raw("rm"), quoted(&input.volume)]))
            } else {
                Ok(Vec::new())
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkInput {
    pub network: String,
    pub driver: Option<String>,
    pub gateway: Option<String>,
    pub ip_range: Option<String>,
    pub ipam_driver: Option<String>,
    pub subnet: Option<String>,
    pub scope: Option<String>,
    pub aux_addresses: BTreeMap<String, String>,
    pub options: Vec<String>,
    pub ipam_options: Vec<String>,
    pub labels: Vec<String>,
    pub ingress: bool,
    pub attachable: bool,
    pub present: bool,
}
impl NetworkInput {
    #[must_use]
    pub fn new(network: impl Into<String>) -> Self {
        Self {
            network: network.into(),
            driver: None,
            gateway: None,
            ip_range: None,
            ipam_driver: None,
            subnet: None,
            scope: None,
            aux_addresses: BTreeMap::new(),
            options: Vec::new(),
            ipam_options: Vec::new(),
            labels: Vec::new(),
            ingress: false,
            attachable: false,
            present: true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Network;
impl StatefulOperation for Network {
    type Input = NetworkInput;
    fn name(&self) -> &'static str {
        "docker.network"
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
            let exists = json_exists(&FactExecutor::new(connector, options).get(&DockerNetwork, &input.network).await?);
            if input.present && !exists {
                let mut bits = vec![raw("docker"), raw("network"), raw("create"), quoted(&input.network)];
                optional(&mut bits, "-d", input.driver.as_deref());
                optional(&mut bits, "--gateway", input.gateway.as_deref());
                optional(&mut bits, "--ip-range", input.ip_range.as_deref());
                optional(&mut bits, "--ipam-driver", input.ipam_driver.as_deref());
                optional(&mut bits, "--subnet", input.subnet.as_deref());
                optional(&mut bits, "--scope", input.scope.as_deref());
                if input.ingress {
                    bits.push(raw("--ingress"));
                }
                if input.attachable {
                    bits.push(raw("--attachable"));
                }
                pairs(&mut bits, "--aux-address", &input.aux_addresses);
                repeated(&mut bits, "--opt", &input.options);
                repeated(&mut bits, "--ipam-opt", &input.ipam_options);
                repeated(&mut bits, "--label", &input.labels);
                Ok(command(bits))
            } else if !input.present && exists {
                Ok(command([raw("docker"), raw("network"), raw("rm"), quoted(&input.network)]))
            } else {
                Ok(Vec::new())
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContainerInput {
    pub container: String,
    pub image: String,
    pub ports: Vec<String>,
    pub networks: Vec<String>,
    pub volumes: Vec<String>,
    pub environment: Vec<String>,
    pub environment_files: Vec<String>,
    pub labels: Vec<String>,
    pub pull_always: bool,
    pub present: bool,
    pub force: bool,
    pub start: bool,
    pub restart_policy: Option<String>,
    pub auto_remove: bool,
    pub mounts: Vec<String>,
    pub privileged: bool,
    pub hostname: Option<String>,
    pub entrypoint: Option<String>,
    pub user: Option<String>,
    pub cpus: Option<f64>,
    pub memory: Option<String>,
    pub extra_args: Vec<String>,
    pub dns: Vec<String>,
    pub container_command: Option<String>,
}
impl ContainerInput {
    #[must_use]
    pub fn new(container: impl Into<String>, image: impl Into<String>) -> Self {
        Self {
            container: container.into(),
            image: image.into(),
            ports: Vec::new(),
            networks: Vec::new(),
            volumes: Vec::new(),
            environment: Vec::new(),
            environment_files: Vec::new(),
            labels: Vec::new(),
            pull_always: false,
            present: true,
            force: false,
            start: true,
            restart_policy: None,
            auto_remove: false,
            mounts: Vec::new(),
            privileged: false,
            hostname: None,
            entrypoint: None,
            user: None,
            cpus: None,
            memory: None,
            extra_args: Vec::new(),
            dns: Vec::new(),
            container_command: None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Container;
impl StatefulOperation for Container {
    type Input = ContainerInput;
    fn name(&self) -> &'static str {
        "docker.container"
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
            let value = FactExecutor::new(connector, options).get(&DockerContainer, &input.container).await?;
            container_commands(input, &value).map_err(Into::into)
        })
    }
}
fn container_commands(input: &ContainerInput, value: &Value) -> Result<Vec<Command>, String> {
    let exists = json_exists(value);
    let running =
        value.as_array().and_then(|v| v.first()).and_then(|v| v.pointer("/State/Status")).and_then(Value::as_str)
            == Some("running");
    let recreate = exists && input.force;
    let mut out = Vec::new();
    if recreate || (exists && !input.present) {
        out.extend(command([raw("docker"), raw("container"), raw("rm"), raw("-f"), quoted(&input.container)]));
    }
    if (input.present && !exists) || recreate {
        if input.image.is_empty() {
            return Err("Docker image not specified".to_owned());
        }
        let mut bits = vec![raw("docker"), raw("container"), raw("create"), raw("--name"), quoted(&input.container)];
        repeated(&mut bits, "--network", &input.networks);
        repeated(&mut bits, "-p", &input.ports);
        repeated(&mut bits, "-v", &input.volumes);
        repeated(&mut bits, "--mount", &input.mounts);
        repeated(&mut bits, "-e", &input.environment);
        repeated(&mut bits, "--env-file", &input.environment_files);
        repeated(&mut bits, "--label", &input.labels);
        if input.pull_always {
            bits.extend([raw("--pull"), raw("always")]);
        }
        optional(&mut bits, "--restart", input.restart_policy.as_deref());
        for (flag, on) in [("--rm", input.auto_remove), ("--privileged", input.privileged)] {
            if on {
                bits.push(raw(flag));
            }
        }
        optional(&mut bits, "--hostname", input.hostname.as_deref());
        optional(&mut bits, "--entrypoint", input.entrypoint.as_deref());
        optional(&mut bits, "--user", input.user.as_deref());
        if let Some(cpus) = input.cpus {
            bits.extend([raw("--cpus"), quoted(cpus.to_string())]);
        }
        optional(&mut bits, "--memory", input.memory.as_deref());
        bits.extend(input.extra_args.iter().map(quoted));
        repeated(&mut bits, "--dns", &input.dns);
        bits.push(quoted(&input.image));
        if let Some(value) = &input.container_command {
            bits.push(raw(value));
        }
        out.extend(command(bits));
    }
    if input.start && (recreate || !running) {
        out.extend(command([raw("docker"), raw("container"), raw("start"), quoted(&input.container)]));
    } else if !input.start && input.present && running {
        out.extend(command([raw("docker"), raw("container"), raw("stop"), quoted(&input.container)]));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
