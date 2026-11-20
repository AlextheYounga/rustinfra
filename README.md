# rustinfra

rustinfra is a Rust-native translation of pyinfra. Infrastructure definitions, inventory,
operations, facts, connection handling, and execution all run in Rust; pyinfra is used only as the
behavioral reference.

Deploys are ordinary typed Rust programs compiled against this crate:

```rust
use std::error::Error;
use std::time::Duration;

use rustinfra::api::{Config, HostSpec, Inventory, InventorySpec, State};
use rustinfra::operations::server::{Shell, ShellCommands};
use rustinfra::operations::{DeployRuntime, ExecutionStrategy, PreparedDeploy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let inventory = Inventory::from_spec(InventorySpec {
        hosts: vec![HostSpec::local()],
        ..InventorySpec::default()
    })?;
    let deploy = PreparedDeploy::prepare(&inventory, |host| {
        host.add(Shell, ShellCommands::from("echo hello from rustinfra"))?;
        Ok(())
    })?;
    let config = Config::default();
    let runtime = DeployRuntime::from_inventory(
        &inventory,
        Duration::from_secs(config.connect_timeout_seconds),
    )?;
    let mut state = State::new(inventory, config);

    runtime.execute(&mut state, &deploy, ExecutionStrategy::Lockstep).await?;
    Ok(())
}
```

Run the included example with:

```sh
cargo run --example local_deploy
```

The translation has reached the documented 85% product-capability milestone and remains in
progress. See `PARITY.md` for the milestone definition and remaining work, and `CONTEXT.md` and
`AGENTS.md` for compatibility requirements and architecture rules.
