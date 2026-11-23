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
fn container_create_and_resources_match_reference_order() {
    let mut input = ContainerInput::new("my app", "nginx:alpine");
    input.ports.push("80:80".to_owned());
    input.environment.push("A=a b".to_owned());
    let commands = container_commands(&input, &Value::Array(Vec::new())).unwrap();
    assert!(
        matches!(&commands[0],Command::Shell(value)if value.raw_value()=="docker container create --name 'my app' -p 80:80 -e 'A=a b' nginx:alpine")
    );
    assert!(matches!(&commands[1],Command::Shell(value)if value.raw_value()=="docker container start 'my app'"));
}

#[test]
fn image_commands_match_tag_digest_force_registry_and_removal_fixtures() {
    let existing = serde_json::json!([{"Id": "sha256:existing"}]);
    let missing = Value::Array(Vec::new());
    let cases = [
        ("nginx:alpine", true, true, &existing, vec!["docker image pull nginx:alpine"]),
        ("ghcr.io/owner/repo:v2.0", true, false, &existing, vec![]),
        ("nginx@sha256:abcd1234567890", true, false, &existing, vec![]),
        ("nginx@sha256:abcd1234567890", true, false, &missing, vec!["docker image pull nginx@sha256:abcd1234567890"]),
        ("nginx:latest", true, false, &existing, vec!["docker image pull nginx:latest"]),
        ("nginx:1.21", true, false, &existing, vec![]),
        ("nginx:1.21", true, false, &missing, vec!["docker image pull nginx:1.21"]),
        (
            "nginx:1.21@sha256:abcd1234567890",
            true,
            false,
            &missing,
            vec!["docker image pull nginx:1.21@sha256:abcd1234567890"],
        ),
        (
            "nginx:1.21@sha256:newdigest123",
            true,
            false,
            &missing,
            vec!["docker image pull nginx:1.21@sha256:newdigest123"],
        ),
        ("nginx:1.21@sha256:correctdigest789", true, false, &existing, vec![]),
        ("nginx", true, false, &existing, vec!["docker image pull nginx"]),
        (
            "myregistry.io:5000/myapp:v1.0",
            true,
            false,
            &missing,
            vec!["docker image pull myregistry.io:5000/myapp:v1.0"],
        ),
        ("nginx:alpine", false, false, &existing, vec!["docker image rm nginx:alpine"]),
        ("nginx:alpine", false, false, &missing, vec![]),
    ];
    for (image, present, force, state, expected) in cases {
        let mut input = ImageInput::new(image);
        input.present = present;
        input.force = force;
        assert_eq!(descriptions(&image_commands(&input, state)), expected, "image {image}");
    }
}

#[test]
fn volume_commands_match_driver_label_option_and_removal_fixtures() {
    let missing = Value::Array(Vec::new());
    let input = VolumeInput::new("nginx_volume");
    assert_eq!(descriptions(&volume_commands(&input, &missing)), ["docker volume create nginx_volume"]);

    let mut driver = input.clone();
    driver.driver = Some("ceph".to_owned());
    assert_eq!(descriptions(&volume_commands(&driver, &missing)), ["docker volume create nginx_volume -d ceph"]);
    let mut labels = input.clone();
    labels.labels = vec!["environment=test".to_owned(), "team=a".to_owned()];
    assert_eq!(
        descriptions(&volume_commands(&labels, &missing)),
        ["docker volume create nginx_volume --label environment=test --label team=a"]
    );
    let mut options = VolumeInput::new("my_volume");
    options.driver = Some("local".to_owned());
    options.options = vec![
        "type=nfs".to_owned(),
        "o=addr=192.168.1.100,rw,nfsvers=4.1".to_owned(),
        "device=:/exported/path".to_owned(),
    ];
    assert_eq!(
        descriptions(&volume_commands(&options, &missing)),
        [
            "docker volume create my_volume -d local --opt type=nfs --opt o=addr=192.168.1.100,rw,nfsvers=4.1 --opt device=:/exported/path"
        ]
    );
    let mut remove = input;
    remove.present = false;
    assert_eq!(
        descriptions(&volume_commands(&remove, &serde_json::json!({"Name": "nginx_volume"}))),
        ["docker volume rm nginx_volume"]
    );
}

#[test]
fn network_commands_match_address_option_flag_and_removal_fixtures() {
    let missing = Value::Array(Vec::new());
    let mut input = NetworkInput::new("nginx");
    input.ingress = true;
    input.attachable = true;
    assert_eq!(
        descriptions(&network_commands(&input, &missing)),
        ["docker network create nginx --ingress --attachable"]
    );

    input = NetworkInput::new("nginx");
    input.driver = Some("macvlan".to_owned());
    input.gateway = Some("192.168.1.1".to_owned());
    input.ip_range = Some("192.168.1.192/27".to_owned());
    input.subnet = Some("192.168.1.1/24".to_owned());
    input.aux_addresses.insert("host".to_owned(), "192.168.1.223".to_owned());
    assert_eq!(
        descriptions(&network_commands(&input, &missing)),
        [
            "docker network create nginx -d macvlan --gateway 192.168.1.1 --ip-range 192.168.1.192/27 --subnet 192.168.1.1/24 --aux-address host=192.168.1.223"
        ]
    );

    input = NetworkInput::new("nginx");
    input.gateway = Some("192.168.0.1".to_owned());
    assert_eq!(
        descriptions(&network_commands(&input, &missing)),
        ["docker network create nginx --gateway 192.168.0.1"]
    );
    input.gateway = None;
    input.ip_range = Some("192.168.0.100/24".to_owned());
    assert_eq!(
        descriptions(&network_commands(&input, &missing)),
        ["docker network create nginx --ip-range 192.168.0.100/24"]
    );
    input.ip_range = None;
    input.options = vec!["opta=A".to_owned(), "optB=B".to_owned()];
    input.ipam_options = vec!["ipam_opta=IPAM_A".to_owned(), "imap_optB=IPAM_B".to_owned()];
    assert_eq!(
        descriptions(&network_commands(&input, &missing)),
        [
            "docker network create nginx --opt opta=A --opt optB=B --ipam-opt ipam_opta=IPAM_A --ipam-opt imap_optB=IPAM_B"
        ]
    );
    input.present = false;
    assert_eq!(descriptions(&network_commands(&input, &missing)), ["docker network rm nginx"]);
}
