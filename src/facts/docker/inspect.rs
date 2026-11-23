use std::error::Error;

use serde_json::Value;

use crate::api::StringCommand;

use super::{json, quote};
use crate::facts::Fact;

const CONTAINERS: &str = "ids=$(docker ps -qa) && [ -n \"$ids\" ] && docker container inspect $ids || echo \"[]\"";
const IMAGES: &str = "ids=$(docker images -q) && [ -n \"$ids\" ] && docker image inspect $ids || echo \"[]\"";
const PLUGINS: &str = "ids=$(docker plugin ls -q) && [ -n \"$ids\" ] && docker plugin inspect $ids || echo \"[]\"";
const NETWORKS: &str = "docker network inspect `docker network ls -q`";
const VOLUMES: &str = "docker volume inspect `docker volume ls -q`";

macro_rules! inspect_fact {
    ($name:ident, $fact_name:literal, $command:expr, $input:ty) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;
        impl Fact for $name {
            type Input = $input;
            type Output = Value;
            fn name(&self) -> &'static str {
                $fact_name
            }
            fn command(&self, input: &Self::Input) -> StringCommand {
                StringCommand::new([$command(input)])
            }
            fn default(&self) -> Self::Output {
                Value::Null
            }
            fn requires_command(&self, _: &Self::Input) -> Option<&str> {
                Some("docker")
            }
            fn process(
                &self,
                output: &[String],
                _: &Self::Input,
            ) -> Result<Self::Output, Box<dyn Error + Send + Sync>> {
                json(output)
            }
        }
    };
}

inspect_fact!(DockerContainers, "docker.DockerContainers", |_: &()| CONTAINERS.to_owned(), ());
inspect_fact!(DockerImages, "docker.DockerImages", |_: &()| IMAGES.to_owned(), ());
inspect_fact!(DockerPlugins, "docker.DockerPlugins", |_: &()| PLUGINS.to_owned(), ());
inspect_fact!(DockerNetworks, "docker.DockerNetworks", |_: &()| NETWORKS.to_owned(), ());
inspect_fact!(DockerVolumes, "docker.DockerVolumes", |_: &()| VOLUMES.to_owned(), ());
inspect_fact!(
    DockerContainer,
    "docker.DockerContainer",
    |id: &String| format!("docker container inspect {} 2>&- || true", quote(id)),
    String
);
inspect_fact!(
    DockerImage,
    "docker.DockerImage",
    |id: &String| format!("docker image inspect {} 2>&- || true", quote(id)),
    String
);
inspect_fact!(
    DockerNetwork,
    "docker.DockerNetwork",
    |id: &String| format!("docker network inspect {} 2>&- || true", quote(id)),
    String
);
inspect_fact!(
    DockerVolume,
    "docker.DockerVolume",
    |id: &String| format!("docker volume inspect {} 2>&- || true", quote(id)),
    String
);
inspect_fact!(
    DockerPlugin,
    "docker.DockerPlugin",
    |id: &String| format!("docker plugin inspect {} 2>&- || true", quote(id)),
    String
);

pub(super) const fn containers_command() -> &'static str {
    CONTAINERS
}
pub(super) const fn images_command() -> &'static str {
    IMAGES
}
pub(super) const fn networks_command() -> &'static str {
    NETWORKS
}
pub(super) const fn volumes_command() -> &'static str {
    VOLUMES
}
