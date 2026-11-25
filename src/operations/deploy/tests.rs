use std::fs;
use std::sync::Arc;

use indexmap::IndexMap;

use super::{OperationOptions, PreparedDeploy};
use crate::api::{HostSpec, Inventory, InventorySpec};
use crate::connectors::{Connector, LocalConnector};
use crate::operations::files::{File, FileInput};
use crate::operations::server::{Shell, ShellCommands};

fn inventory() -> Inventory {
    Inventory::from_spec(InventorySpec {
        hosts: vec![HostSpec::ssh("one"), HostSpec::ssh("two")],
        ..InventorySpec::default()
    })
    .unwrap()
}

#[test]
fn preparation_merges_matching_calls_and_keeps_host_specific_paths() {
    let deploy = PreparedDeploy::prepare(&inventory(), |host| {
        let command = format!("echo {}", host.host().name());
        host.add(Shell, ShellCommands::from(command))?;
        if host.host().name() == "two" {
            host.add(Shell, ShellCommands::from("echo only-two"))?;
        }
        Ok(())
    })
    .unwrap();

    assert_eq!(deploy.plan.operations.len(), 2);
    assert_eq!(deploy.plan.operations[0].hosts.len(), 2);
    assert_eq!(deploy.plan.operations[1].hosts.iter().map(String::as_str).collect::<Vec<_>>(), vec!["two"]);
}

#[test]
fn run_once_is_attached_only_to_the_first_host_that_reaches_the_call() {
    let deploy = PreparedDeploy::prepare(&inventory(), |host| {
        host.add_with_options(
            Shell,
            ShellCommands::from("echo once"),
            OperationOptions { run_once: true, ..OperationOptions::default() },
        )?;
        Ok(())
    })
    .unwrap();

    assert_eq!(deploy.plan.operations.len(), 1);
    assert_eq!(deploy.plan.operations[0].hosts.iter().map(String::as_str).collect::<Vec<_>>(), vec!["one"]);
}

#[tokio::test]
async fn preview_classifies_current_host_changes_without_applying_them() {
    let inventory =
        Inventory::from_spec(InventorySpec { hosts: vec![HostSpec::local()], ..InventorySpec::default() }).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing");
    let missing = directory.path().join("missing");
    fs::write(&existing, "already here").unwrap();
    let deploy = PreparedDeploy::prepare(&inventory, |host| {
        host.add_stateful(
            File,
            FileInput::new(existing.to_string_lossy()),
            OperationOptions { name: Some("existing file".to_owned()), ..OperationOptions::default() },
        )?;
        host.add_stateful(
            File,
            FileInput::new(missing.to_string_lossy()),
            OperationOptions { name: Some("missing file".to_owned()), ..OperationOptions::default() },
        )?;
        Ok(())
    })
    .unwrap();
    let connector: Arc<dyn Connector> = Arc::new(LocalConnector);
    let connectors = IndexMap::from([("@local".to_owned(), connector)]);

    let preview = deploy.preview(&connectors, 1).await.unwrap();

    assert_eq!(preview.operations[0].name, "existing file");
    assert_eq!(preview.operations[0].change_hosts, Vec::<String>::new());
    assert_eq!(preview.operations[0].no_change_hosts, ["@local"]);
    assert_eq!(preview.operations[1].name, "missing file");
    assert_eq!(preview.operations[1].change_hosts, ["@local"]);
    assert!(!missing.exists());
}
