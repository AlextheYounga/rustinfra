use std::error::Error;
use std::time::Duration;

use rustinfra::api::{Config, HostSpec, Inventory, InventorySpec, State};
use rustinfra::operations::server::{Shell, ShellCommands};
use rustinfra::operations::{DeployRuntime, ExecutionStrategy, PreparedDeploy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let inventory = Inventory::from_spec(InventorySpec { hosts: vec![HostSpec::local()], ..InventorySpec::default() })?;
    let deploy = PreparedDeploy::prepare(&inventory, |host| {
        host.add(Shell, ShellCommands::from("echo hello from rustinfra"))?;
        Ok(())
    })?;
    let config = Config::default();
    let runtime = DeployRuntime::from_inventory(&inventory, Duration::from_secs(config.connect_timeout_seconds))?;
    let mut state = State::new(inventory, config);

    runtime.execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await?;
    Ok(())
}
