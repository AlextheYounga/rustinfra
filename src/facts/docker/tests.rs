use serde_json::json;

use super::*;
use crate::facts::Fact;

#[test]
fn inspect_facts_preserve_json_and_quote_object_ids() {
    let id = "id with ' quote".to_owned();
    assert_eq!(
        DockerContainer.command(&id).raw_value(),
        "docker container inspect 'id with '\"'\"' quote' 2>&- || true"
    );
    assert_eq!(DockerContainer.process(&[r#"{"hello":"world"}"#.to_owned()], &id).unwrap(), json!({"hello": "world"}));
    assert_eq!(
        DockerContainers.command(&()).raw_value(),
        "ids=$(docker ps -qa) && [ -n \"$ids\" ] && docker container inspect $ids || echo \"[]\""
    );
    assert_eq!(DockerPlugin.command(&"myid".to_owned()).raw_value(), "docker plugin inspect myid 2>&- || true");
    assert_eq!(
        DockerImages.command(&()).raw_value(),
        "ids=$(docker images -q) && [ -n \"$ids\" ] && docker image inspect $ids || echo \"[]\""
    );
    assert_eq!(
        DockerPlugins.command(&()).raw_value(),
        "ids=$(docker plugin ls -q) && [ -n \"$ids\" ] && docker plugin inspect $ids || echo \"[]\""
    );
    let images = DockerImages.process(&[r#"[{"Id":"sha256:img1"}]"#.to_owned()], &()).unwrap();
    assert_eq!(images[0]["Id"], "sha256:img1");
    let plugins = DockerPlugins.process(&[r#"[{"Id":"plugin1","Enabled":true}]"#.to_owned()], &()).unwrap();
    assert_eq!(plugins[0]["Enabled"], true);
}

#[test]
fn derived_container_facts_handle_values_and_nulls() {
    let output = vec![r#"[{"Id":"abc123","Config":{"Env":["PATH=/usr/bin","DEBUG=true","EMPTY="],"Labels":{"role":"web"}},"Mounts":[{"Type":"bind"}],"NetworkSettings":{"Networks":{"bridge":{"IPAddress":"1.2.3.4"}},"Ports":{"80/tcp":[{"HostPort":"8080"}]} }},{"Id":"def456","Config":{"Env":null,"Labels":null}}]"#.to_owned()];
    let envs = DockerContainerEnvs.process(&output, &()).unwrap();
    assert_eq!(envs["abc123"]["PATH"], "/usr/bin");
    assert_eq!(envs["abc123"]["EMPTY"], "");
    assert!(envs["def456"].is_empty());
    assert_eq!(DockerContainerLabels.process(&output, &()).unwrap()["abc123"]["role"], "web");
    assert_eq!(DockerContainerMounts.process(&output, &()).unwrap()["abc123"][0]["Type"], "bind");
    assert_eq!(DockerContainerNetworks.process(&output, &()).unwrap()["abc123"]["bridge"]["IPAddress"], "1.2.3.4");
    assert_eq!(DockerContainerPorts.process(&output, &()).unwrap()["abc123"]["80/tcp"][0]["HostPort"], "8080");
}

#[test]
fn derived_image_and_storage_facts_match_reference_fallbacks() {
    let images = vec![r#"[{"Id":"img1","Config":{"Labels":null},"ContainerConfig":{"Labels":{"old":"yes"}},"RootFS":{"Layers":["sha256:a"]}},{"Id":"img2","Config":{"Labels":{}}}]"#.to_owned()];
    assert_eq!(DockerImageLabels.process(&images, &()).unwrap()["img1"]["old"], "yes");
    assert_eq!(DockerImageLayers.process(&images, &()).unwrap()["img1"], ["sha256:a"]);
    let networks = vec![r#"[{"Id":"net1","Labels":{"scope":"test"}}]"#.to_owned()];
    assert_eq!(DockerNetworkLabels.process(&networks, &()).unwrap()["net1"]["scope"], "test");
    let volumes = vec![r#"[{"Name":"data","Labels":null}]"#.to_owned()];
    assert!(DockerVolumeLabels.process(&volumes, &()).unwrap()["data"].is_empty());
}

#[test]
fn command_specific_facts_parse_reference_formats() {
    assert_eq!(DockerVersion.process(&["Docker version 29.3.1\n".to_owned()], &()).unwrap(), "Docker version 29.3.1");
    let changes = DockerContainerFsChanges
        .process(&["A /new\n".to_owned(), "C /changed file".to_owned(), "invalid".to_owned()], &"abc".to_owned())
        .unwrap();
    assert_eq!(
        changes,
        [
            DockerFsChange { change: "A".to_owned(), path: "/new".to_owned() },
            DockerFsChange { change: "C".to_owned(), path: "/changed file".to_owned() }
        ]
    );
    let processes = DockerContainerProcesses
        .process(&["UID PID CMD".to_owned(), "root 123 nginx: master process".to_owned()], &"abc".to_owned())
        .unwrap();
    assert_eq!(processes[0]["CMD"], "nginx: master process");
    let stats =
        DockerContainerStats.process(&[r#"{"ID":"abc","CPUPerc":"0.01%"}"#.to_owned(), "".to_owned()], &()).unwrap();
    assert_eq!(stats[0]["ID"], "abc");
    let image = "sha256:img1".to_owned();
    assert_eq!(
        DockerImageHistory.command(&image).raw_value(),
        "docker image history --no-trunc --format '{{json .}}' sha256:img1 2>&- || true"
    );
    let history =
        DockerImageHistory.process(&[r#"{"ID":"sha256:layer1","CreatedBy":"/bin/sh"}"#.to_owned()], &image).unwrap();
    assert_eq!(history[0]["CreatedBy"], "/bin/sh");
}

#[test]
fn docker_auths_returns_keys_and_tolerates_malformed_config() {
    let auths = DockerAuths.process(&[r#"{"auths":{"docker.io":{},"registry:5000":{}}}"#.to_owned()], &()).unwrap();
    assert_eq!(auths, ["docker.io", "registry:5000"]);
    assert!(DockerAuths.process(&["{}".to_owned()], &()).unwrap().is_empty());
    assert!(DockerAuths.process(&["not json".to_owned()], &()).unwrap().is_empty());
}
