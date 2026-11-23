use super::*;

fn directory() -> PathInfo {
    PathInfo::Present(FileInfo {
        user: "root".to_owned(),
        group: "root".to_owned(),
        mode: 755,
        accessed_at: None,
        modified_at: None,
        changed_at: None,
        size: 0,
        link_target: None,
    })
}
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
fn config_handles_scopes_multi_values_and_safe_quoting() {
    let mut input = ConfigInput::new("user.name; whoami", "Anon E. Mouse");
    input.repo = Some("/my repo".to_owned());
    assert_eq!(
        descriptions(&config_commands(&input, &IndexMap::new())),
        ["cd '/my repo' && git config --local 'user.name; whoami' \"Anon E. Mouse\""]
    );
    input.multi_value = true;
    let current = [(input.key.clone(), vec![input.value.clone()])].into_iter().collect();
    assert!(config_commands(&input, &current).is_empty());
}

#[test]
fn bare_repo_initializes_removes_and_applies_ownership() {
    let mut input = BareRepoInput::new("/home/my repo");
    input.user = Some("fake".to_owned());
    assert_eq!(
        descriptions(&bare_repo_commands(&input, &directory(), &PathInfo::Missing).unwrap()),
        ["git init --bare '/home/my repo'", "chown -R fake '/home/my repo'"]
    );
    input.present = false;
    assert_eq!(
        descriptions(&bare_repo_commands(&input, &directory(), &PathInfo::Missing).unwrap()),
        ["rm -rf '/home/my repo'"]
    );
}

#[test]
fn worktree_create_pull_and_remove_match_reference_order() {
    let mut input = WorktreeInput::new("/home/my worktree");
    input.repo = Some("/home/main repo".to_owned());
    input.new_branch = Some("my branch".to_owned());
    input.user = Some("app".to_owned());
    let state = WorktreeState { repo: directory(), ..WorktreeState::default() };
    let created = descriptions(&worktree_commands(&input, &state).unwrap());
    assert_eq!(created[0], "cd '/home/main repo' && git worktree add -b 'my branch' '/home/my worktree'");
    assert_eq!(created.len(), 5);

    input.present = false;
    input.force = true;
    let state = WorktreeState { path: directory(), ..WorktreeState::default() };
    assert_eq!(
        descriptions(&worktree_commands(&input, &state).unwrap()),
        ["cd '/home/my worktree' && git worktree remove . --force"]
    );
}
