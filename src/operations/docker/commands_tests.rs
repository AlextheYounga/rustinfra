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
fn compose_matches_up_down_file_project_pull_and_v1_fixtures() {
    let input = ComposeInput::new("/srv/app");
    assert_eq!(
        descriptions(&Compose.commands(&input)),
        ["docker compose --project-directory /srv/app up -d --remove-orphans"]
    );

    let mut down = input.clone();
    down.present = false;
    assert_eq!(
        descriptions(&Compose.commands(&down)),
        ["docker compose --project-directory /srv/app down --remove-orphans"]
    );
    down.remove_volumes = true;
    assert_eq!(
        descriptions(&Compose.commands(&down)),
        ["docker compose --project-directory /srv/app down -v --remove-orphans"]
    );

    let mut full = input.clone();
    full.build = true;
    full.force_recreate = true;
    assert_eq!(
        descriptions(&Compose.commands(&full)),
        ["docker compose --project-directory /srv/app up -d --build --force-recreate --remove-orphans"]
    );
    full = input.clone();
    full.files = vec!["docker-compose.yml".to_owned(), "docker-compose.prod.yml".to_owned()];
    assert_eq!(
        descriptions(&Compose.commands(&full)),
        [
            "docker compose --project-directory /srv/app -f docker-compose.yml -f docker-compose.prod.yml up -d --remove-orphans"
        ]
    );
    full.files.truncate(1);
    assert_eq!(
        descriptions(&Compose.commands(&full)),
        ["docker compose --project-directory /srv/app -f docker-compose.yml up -d --remove-orphans"]
    );

    full = input.clone();
    full.project_name = Some("app".to_owned());
    full.pull = Some("always".to_owned());
    assert_eq!(
        descriptions(&Compose.commands(&full)),
        ["docker compose --project-directory /srv/app --project-name app up -d --pull always --remove-orphans"]
    );
    full = input;
    full.compose_command = "docker-compose".to_owned();
    assert_eq!(
        descriptions(&Compose.commands(&full)),
        ["docker-compose --project-directory /srv/app up -d --remove-orphans"]
    );
    assert!(!Compose.is_idempotent());
}

#[test]
fn container_exec_matches_argv_flags_shell_quoting_and_validation_fixtures() {
    let mut argv = ContainerExecInput::new("app", "unused");
    argv.command = ExecCommand::Arguments(vec!["touch".to_owned(), "/srv/ready".to_owned()]);
    argv.user = Some("appuser".to_owned());
    argv.workdir = Some("/srv".to_owned());
    argv.env.insert("STAGE".to_owned(), "prod".to_owned());
    assert_eq!(
        descriptions(&container_exec_commands(&argv).unwrap()),
        ["docker exec --user appuser --workdir /srv --env STAGE=prod app touch /srv/ready"]
    );

    let mut flags = ContainerExecInput::new("app", "unused");
    flags.command = ExecCommand::Arguments(vec!["ls".to_owned()]);
    flags.detach = true;
    flags.privileged = true;
    flags.tty = true;
    assert_eq!(
        descriptions(&container_exec_commands(&flags).unwrap()),
        ["docker exec --detach --privileged --tty app ls"]
    );

    let mut quoted = ContainerExecInput::new("x; reboot", "unused");
    quoted.command = ExecCommand::Arguments(vec!["id".to_owned()]);
    assert_eq!(descriptions(&container_exec_commands(&quoted).unwrap()), ["docker exec 'x; reboot' id"]);
    let shell = ContainerExecInput::new("app", "./manage.py migrate 2>&1 | tee /var/log/migrate.log");
    assert_eq!(
        descriptions(&container_exec_commands(&shell).unwrap()),
        ["docker exec app sh -c './manage.py migrate 2>&1 | tee /var/log/migrate.log'"]
    );
    assert_eq!(
        container_exec_commands(&ContainerExecInput::new("app", "")).unwrap_err(),
        "docker.container_exec requires a command"
    );
    assert!(!ContainerExec.is_idempotent());
}

#[test]
fn prune_matches_default_all_volume_and_filter_fixtures() {
    assert_eq!(descriptions(&Prune.commands(&PruneInput::default())), ["docker system prune -f"]);
    assert_eq!(
        descriptions(&Prune.commands(&PruneInput { all: true, volumes: true, filter: None })),
        ["docker system prune -a --volumes -f"]
    );
    assert_eq!(
        descriptions(&Prune.commands(&PruneInput {
            all: false,
            volumes: false,
            filter: Some("until=2160h".to_owned()),
        })),
        ["docker system prune --filter=until=2160h -f"]
    );
    assert!(!Prune.is_idempotent());
}
