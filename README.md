# rustinfra

Infrastructure deserves a real programming language. rustinfra brings pyinfra’s proven deployment
model to Rust: typed deploys, explicit behavior, and first-class parallel execution—without a Python
runtime or a second configuration language.

The translation is deliberate: pyinfra remains the behavioral reference, not a runtime dependency.
We preserve the workflows and semantics that operators rely on, then use Rust to make them clear,
composable, and safe to run concurrently.

## A deploy is Rust

Inventory, operations, and execution are ordinary Rust code:

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

Run it:

```sh
cargo run --example local_deploy
```

Before applying operations, rustinfra reads each host's current state and displays the proposed
changes. Interactive terminals get a Docker Compose-inspired display with animated connect, plan,
apply, and disconnect phases. Command output remains visible with host and operation prefixes, and
each host operation reports `Success`, `No changes`, or `Failed`. Redirected output and CI logs use a
stable plain-text format. Deploys that need another presentation can provide a `DeployReporter`;
`NullReporter` disables output.

## Where it stands

rustinfra has reached the **85% product-capability milestone**. That means the core deployment
workflows are runnable; it does **not** mean 85% of pyinfra’s operation and fact catalog has been
ported. The remaining work is real, and the parity document says what it is.

Today, rustinfra covers the deploy runtime and SSH foundation, filesystem convergence, Debian and
RPM and language packages, systemd, users, templates, line editing, sysctl, Git, archive extraction,
cron, pip environments, APT sources, iptables, locales, and security limits.

For the milestone definitions, read [PARITY.md](PARITY.md). Track the work toward strict behavioral
and feature parity in [PARITY_CHECKLIST.md](PARITY_CHECKLIST.md). Compatibility rules and
architecture live in [CONTEXT.md](CONTEXT.md) and [AGENTS.md](AGENTS.md).
