use indexmap::IndexMap;

use super::venv::venv_create_commands;
use super::{LinkMode, PackagesInput, ToolsInput, VenvInput, package_commands, tool_commands};
use crate::api::Command;

fn descriptions(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Shell(value) => Some(value.raw_value()),
            _ => None,
        })
        .collect()
}

#[test]
fn packages_handle_requirements_presence_removal_and_latest() {
    let installed = IndexMap::from([
        ("ruff".to_owned(), vec!["0.14.4".to_owned()]),
        ("mypy".to_owned(), vec!["1.17.1".to_owned()]),
    ]);
    let mut input = PackagesInput::new(["ruff", "mypy", "pyrefly"]);
    input.extra_arguments = vec!["--no-cache".to_owned()];
    assert_eq!(
        descriptions(&package_commands(&input, &installed)),
        ["uv pip install --no-progress --no-cache pyrefly"]
    );
    input.present = false;
    assert_eq!(
        descriptions(&package_commands(&input, &installed)),
        ["uv pip uninstall --no-progress --no-cache ruff mypy"]
    );
    input.packages.clear();
    input.requirements = Some("requirements file.txt".to_owned());
    input.latest = true;
    input.present = true;
    assert_eq!(
        descriptions(&package_commands(&input, &installed)),
        ["uv pip install --upgrade --no-progress --no-cache -r 'requirements file.txt'"]
    );
}

#[test]
fn tools_are_converged_individually_and_pep508_values_are_normalized() {
    let installed = IndexMap::from([("mypy".to_owned(), vec!["1.17.1".to_owned()])]);
    let input = ToolsInput::new(["ruff==0.14.4", "mypy", "sphinx >= 8.2"]);
    assert_eq!(
        descriptions(&tool_commands(&input, &installed)),
        ["uv tool install --no-progress ruff==0.14.4", "uv tool install --no-progress 'sphinx>=8.2'"]
    );
}

#[test]
fn package_removal_preserves_version_url_and_legacy_requirements() {
    let installed = IndexMap::from([
        ("ruff".to_owned(), vec!["0.14.4".to_owned()]),
        ("demo".to_owned(), vec!["1.0".to_owned()]),
        ("broken requirement".to_owned(), vec!["unknown".to_owned()]),
    ]);
    let mut input = PackagesInput::new(["ruff==0.14.4", "demo @ https://example.com/demo.whl", "broken requirement"]);
    input.present = false;
    assert_eq!(
        descriptions(&package_commands(&input, &installed)),
        ["uv pip uninstall --no-progress ruff==0.14.4 https://example.com/demo.whl 'broken requirement'"]
    );
}

#[test]
fn venv_creation_preserves_option_order_and_quotes_values() {
    let mut input = VenvInput::new("/somewhere/useful project");
    input.clear_existing = true;
    input.link_mode = Some(LinkMode::Symlink);
    input.site_packages = true;
    input.extra_arguments = vec!["--offline".to_owned()];
    assert_eq!(
        descriptions(&venv_create_commands(&input, false)),
        ["uv venv --clear --link-mode symlink --site-packages --offline '/somewhere/useful project'"]
    );
    assert!(venv_create_commands(&input, true).is_empty());
}
