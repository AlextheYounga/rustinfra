use super::*;

#[test]
fn compose_exec_and_prune_quote_values() {
    let mut compose = ComposeInput::new("/srv/my app");
    compose.files.push("prod file.yml".to_owned());
    assert!(
        matches!(&Compose.commands(&compose)[0],Command::Shell(value)if value.raw_value().contains("'/srv/my app'")&&value.raw_value().contains("'prod file.yml'"))
    );
    let exec = ContainerExecInput::new("my app", "echo $HOME; reboot");
    assert!(
        matches!(&ContainerExec.commands(&exec)[0],Command::Shell(value)if value.raw_value().ends_with("'echo $HOME; reboot'"))
    );
    assert!(
        matches!(&Prune.commands(&PruneInput{all:true,volumes:true,filter:Some("until=24h".to_owned())})[0],Command::Shell(value)if value.raw_value()=="docker system prune -a --filter=until=24h --volumes -f")
    );
}
