# rustinfra

Infrastructure deserves a real programming language. rustinfra brings pyinfra’s proven deployment
model to Rust: typed deploys, explicit behavior, and first-class parallel execution—without a Python
runtime or a second configuration language.

The translation is deliberate: pyinfra remains the behavioral reference, not a runtime dependency.
We preserve the workflows and semantics that operators rely on, then use Rust to make them clear,
composable, and safe to run concurrently.

## Getting started

Use a current stable Rust toolchain and a POSIX shell. For remote deployments, you'll also need
SSH access to your hosts.

```sh
git clone https://github.com/AlextheYounga/rustinfra.git
cd rustinfra
cargo run --example local_deploy
```

This example runs `echo hello from rustinfra` on your machine. No Python runtime, remote agent, or
local pyinfra checkout is required. Shell operations run their commands each time; use desired-state
operations for idempotent convergence.

Rustinfra is currently a **library used by compiled Rust deploys**, not a finished standalone CLI.
`cargo run` alone does not execute a deploy; use the example or your own Rust application.

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

The code above is the workflow in [`examples/local_deploy.rs`](examples/local_deploy.rs). To use
rustinfra in your own application, add it and a Tokio runtime:

```sh
cargo add rustinfra
cargo add tokio --features macros,rt-multi-thread
```

Replace `HostSpec::local()` with `HostSpec::ssh("web-01")` for an SSH target; the name can be an
OpenSSH host alias. Local and SSH targets are supported.

### Desired-state operations

For a file that should exist, use a stateful operation inside the preparation closure:

```rust
host.add_stateful(
    rustinfra::operations::files::File,
    rustinfra::operations::files::FileInput::new("/tmp/rustinfra-example"),
    rustinfra::operations::OperationOptions::default(),
)?;
```

Unlike the shell example, this changes the target filesystem. The operation reads current state
and produces no commands when the requested state is already satisfied.

## Plan, apply, and report

Before applying operations, rustinfra reads each host's current state and displays the proposed
changes. Interactive terminals get a Docker Compose-inspired display with animated connect, plan,
apply, and disconnect phases. Command output remains visible with host and operation prefixes, and
each host operation reports `Success`, `No changes`, or `Failed`. Redirected output and CI logs use a
stable plain-text format. Deploys that need another presentation can provide a `DeployReporter`;
`NullReporter` disables output.

Interactive output keeps text left-aligned and places phase spinners and result symbols in a shared
right-side column:

```text
Connected 1/1                                                  ✓
Planned 1/1                                                    ✓
 server.shell             change: @local                       ✔
Applying 0/1 server.shell on @local                            ⠹
 @local       server.shell             | hello from rustinfra
 @local       server.shell             Success                 ✔
Applied 1/1                                                    ✓
Disconnected 1/1                                               ✓
Deployment complete
```

- `✓` marks a finished phase; host results use `✔` for success, `─` for no change, `✘` for failure,
  and `!` for an ignored failure.
- Default reporting goes to **stderr**. Redirect with `2>deploy.log` or capture both streams with
  `>deploy.log 2>&1`.
- Redirected output and `TERM=dumb` use plain text.
- Operations read current state again when applied, so earlier changes inform later decisions.

To disable presentation, configure the runtime before calling `execute`:

```rust
let runtime = runtime.with_reporter(std::sync::Arc::new(
    rustinfra::operations::NullReporter,
));
```

Use `TextReporter::new(writer)` to choose an explicit plain-text destination, or implement
`DeployReporter` for custom presentation.

## Execution model

`PreparedDeploy::prepare` evaluates the Rust closure for each host, registers operations, and
builds their dependency order. `DeployRuntime::execute` then connects, previews changes, applies
operations, and disconnects. Preparation and preview do not apply the generated operation commands.

`ExecutionStrategy::Lockstep` finishes an operation across applicable hosts before starting the
next. Serial and per-host/no-wait strategies are also available. `Config::parallel` bounds host
concurrency; `OperationOptions` supplies naming, run-once, serial, and per-operation parallel controls.
Commands within a host operation stay ordered.

## What you can manage

Rustinfra includes operations for files and templates, users and system settings, package managers,
service managers, Git, cron, containers, databases, and language tooling. Deploy locally or over a
persistent SSH connection, with typed inputs and bounded parallel execution across hosts.

## Development and documentation

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc
cargo doc --no-deps
```

- [Architecture and context](CONTEXT.md): lifecycle, boundaries, dependencies, and open decisions.
- [Contributor guide](AGENTS.md): source-first translation, testing, and code conventions.

Rustinfra is MIT licensed. Pyinfra is the behavioral reference, not a runtime dependency; copied
reference material retains its required attribution.
