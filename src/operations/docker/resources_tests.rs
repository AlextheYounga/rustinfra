use super::*;

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
