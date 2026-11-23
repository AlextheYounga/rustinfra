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
fn build_matches_context_tag_cache_buildx_and_validation_fixtures() {
    let input = BuildInput::new("/srv/app");
    assert_eq!(descriptions(&build_commands(&input, false).unwrap()), ["docker build /srv/app"]);

    let mut tagged = input.clone();
    tagged.tags.push("app:1.0".to_owned());
    assert_eq!(descriptions(&build_commands(&tagged, true).unwrap()), Vec::<String>::new());
    assert_eq!(descriptions(&build_commands(&tagged, false).unwrap()), ["docker build -t app:1.0 /srv/app"]);
    tagged.force = true;
    assert_eq!(descriptions(&build_commands(&tagged, true).unwrap()), ["docker build -t app:1.0 /srv/app"]);

    let mut multi = input.clone();
    multi.tags = vec!["app:1.0".to_owned(), "app:latest".to_owned()];
    assert!(build_commands(&multi, true).unwrap().is_empty());
    assert_eq!(
        descriptions(&build_commands(&multi, false).unwrap()),
        ["docker build -t app:1.0 -t app:latest /srv/app"]
    );

    let mut dockerfile = input.clone();
    dockerfile.tags.push("app:prod".to_owned());
    dockerfile.dockerfile = Some("docker/Dockerfile.prod".to_owned());
    assert_eq!(
        descriptions(&build_commands(&dockerfile, false).unwrap()),
        ["docker build -t app:prod -f docker/Dockerfile.prod /srv/app"]
    );

    let mut options = input.clone();
    options.tags.push("app:1.0".to_owned());
    options.target = Some("runtime".to_owned());
    options.platform = Some("linux/amd64".to_owned());
    options.pull = true;
    options.no_cache = true;
    assert_eq!(
        descriptions(&build_commands(&options, false).unwrap()),
        ["docker build -t app:1.0 --target runtime --platform linux/amd64 --pull --no-cache /srv/app"]
    );

    options = input.clone();
    options.tags.push("app:1.0".to_owned());
    options.build_args.insert("VERSION".to_owned(), "1.0".to_owned());
    options.build_args.insert("COMMIT".to_owned(), "abc123".to_owned());
    options.labels.insert("org.opencontainers.image.source".to_owned(), "https://example.com/app".to_owned());
    assert_eq!(
        descriptions(&build_commands(&options, false).unwrap()),
        [
            "docker build -t app:1.0 --build-arg VERSION=1.0 --build-arg COMMIT=abc123 --label org.opencontainers.image.source=https://example.com/app /srv/app"
        ]
    );

    options = input.clone();
    options.tags.push("app:1.0".to_owned());
    options.network = Some("host".to_owned());
    options.cache_from = vec!["app:builder".to_owned(), "app:latest".to_owned()];
    options.secrets.push("id=npmrc,src=/root/.npmrc".to_owned());
    assert_eq!(
        descriptions(&build_commands(&options, false).unwrap()),
        [
            "docker build -t app:1.0 --network host --cache-from app:builder --cache-from app:latest --secret id=npmrc,src=/root/.npmrc /srv/app"
        ]
    );

    options = input.clone();
    options.tags.push("registry.io/app:cache".to_owned());
    options.build_command = "docker buildx build".to_owned();
    options.cache_from.push("type=registry,ref=registry.io/app:cache".to_owned());
    options.cache_to = vec!["type=registry,ref=registry.io/app:cache,mode=max".to_owned(), "type=inline".to_owned()];
    assert_eq!(
        descriptions(&build_commands(&options, false).unwrap()),
        [
            "docker buildx build -t registry.io/app:cache --cache-from type=registry,ref=registry.io/app:cache --cache-to type=registry,ref=registry.io/app:cache,mode=max --cache-to type=inline /srv/app"
        ]
    );

    options = input;
    options.tags.push("registry.io/app:arm64".to_owned());
    options.build_command = "docker buildx build".to_owned();
    options.builder = Some("multiarch".to_owned());
    options.platform = Some("linux/arm64".to_owned());
    assert_eq!(
        descriptions(&build_commands(&options, false).unwrap()),
        ["docker buildx build --builder multiarch -t registry.io/app:arm64 --platform linux/arm64 /srv/app"]
    );
    assert_eq!(build_commands(&BuildInput::new(""), false).unwrap_err(), "docker.build requires a context path");
}

#[test]
fn login_matches_force_noop_hub_private_and_masking_fixtures() {
    let mut input =
        LoginInput { username: "ci".to_owned(), password: HiddenValue::new("s3cret"), server: None, force: false };
    assert!(login_commands(&input, true).is_empty());
    let command = login_commands(&input, false);
    let Command::Shell(command) = &command[0] else { panic!("expected shell command") };
    assert_eq!(command.raw_value(), "printf '%s' s3cret | docker login --username ci --password-stdin");
    assert_eq!(command.masked_value(), "printf '%s' '*MASKED*' | docker login --username ci --password-stdin");

    input.server = Some("myregistry.io:5000".to_owned());
    assert!(login_commands(&input, true).is_empty());
    let command = login_commands(&input, false);
    let Command::Shell(command) = &command[0] else { panic!("expected shell command") };
    assert_eq!(
        command.raw_value(),
        "printf '%s' s3cret | docker login --username ci --password-stdin myregistry.io:5000"
    );
    input.password = HiddenValue::new("s!%^&$");
    let command = login_commands(&input, false);
    let Command::Shell(command) = &command[0] else { panic!("expected shell command") };
    assert_eq!(
        command.raw_value(),
        "printf '%s' 's!%^&$' | docker login --username ci --password-stdin myregistry.io:5000"
    );
    assert!(command.masked_value().contains("'*MASKED*'"));
    input.force = true;
    assert!(!login_commands(&input, true).is_empty());
}

#[test]
fn logout_matches_hub_private_and_unauthenticated_fixtures() {
    let mut input = LogoutInput::default();
    assert_eq!(descriptions(&logout_commands(&input, true)), ["docker logout"]);
    input.server = Some("myregistry.io:5000".to_owned());
    assert_eq!(descriptions(&logout_commands(&input, true)), ["docker logout myregistry.io:5000"]);
    assert!(logout_commands(&input, false).is_empty());
}
