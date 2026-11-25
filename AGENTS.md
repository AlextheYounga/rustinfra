# rustinfra Agent Guide

## Mission

Translate pyinfra into Rust while preserving pyinfra's behavior, architecture, terminology, and
feature set. First-class parallel execution is the intentional enhancement. It is not permission to
redesign unrelated parts of the product.

The locally symlinked `pyinfra/` directory is the reference implementation. At the time this guide
was written, it points to pyinfra's `3.x` branch at commit `cd95d5a3`. Treat the checked-out source,
tests, and documentation together as the specification. If they disagree, investigate the actual
behavior and tests rather than guessing.

Do not modify files under `pyinfra/` as part of the Rust port.

## Current Implementation and Documentation

Deploys currently run as compiled Rust applications using the library; `src/main.rs` is empty.
`examples/local_deploy.rs` is the runnable getting-started reference. `DeployRuntime::from_inventory`
constructs local and SSH connectors, not every built-in connector in the parity target.

Keep documentation claims tied to evidence:

- `README.md` owns the user-facing introduction, setup, deploy example, reporting, and limitations.
- `CONTEXT.md` owns architecture, selected dependencies, and open decisions.
- `PARITY.md` summarizes current status and preserves historical capability milestones.
- `PARITY_CHECKLIST.md` owns remaining qualification and release gates.
- `PARITY_REPORT.md` is generated from `parity/*.tsv`; regenerate it with
  `python scripts/parity_report.py`, never edit its counts by hand.

Catalog symbol presence is not behavioral parity. Do not turn a module inventory checkmark or an
older capability milestone into a full-parity claim. Reference-fixture qualification requires linked
test evidence; the report check verifies references, while the Rust suite executes the tests.

Keep terminal formatting in `src/operations/report/compose.rs`, behind `DeployReporter`. Preserve
plain-text fallback, custom reporters, `NullReporter`, and host/operation attribution. Phase-finished
events describe lifecycle completion, not success. Do not infer successful execution from a rendered
checkmark or summary; preserve the returned runtime error.

## Primary Rule: Translate, Do Not Redesign

Pyinfra has already solved the product and domain problems. Preserve those solutions unless a
critical Rust limitation makes a direct translation impractical.

- Keep pyinfra's observable behavior, execution semantics, edge cases, naming, and feature set.
- Keep its major concepts and boundaries recognizable: inventory, hosts, host/group data,
  connectors, facts, operations, commands, deploy state, callbacks/events, and results.
- Translate logic one-to-one by default.
- Do not replace an established pyinfra solution merely because another design seems cleaner.
- Do not silently remove uncommon behavior or compatibility handling.
- Surface any meaningful behavioral or architectural deviation for human agreement before making
  it.

Simplification means expressing the same behavior with substantially less or clearer code. It does
not mean simplifying the product. Simplify when:

- Rust's type system, ownership, enums, traits, iterators, or standard library directly replace
  Python-specific machinery.
- A mature crate provides the same low-level capability and semantics.
- The original logic can plainly be shortened without losing behavior, diagnostics, or edge cases.

Do not create abstractions, DSLs, compatibility layers, or extension points without a current
pyinfra behavior that requires them.

## Rust Translation Principles

Use idiomatic Rust where it is the natural equivalent of the Python implementation:

- Exceptions become typed `Result` errors.
- Known dictionary shapes become typed structs or enums.
- Python base classes and protocols become traits where dynamic behavior is required.
- Closed sets of variants become enums rather than trait objects.
- Decorators and runtime introspection become explicit constructors, traits, or registries.
- Context-local globals become explicit typed context unless task-local context is necessary to
  preserve behavior.
- Resource cleanup uses ownership and RAII.
- Concurrent work uses structured concurrency rather than shared mutable global state.

Facts must have typed Rust inputs and outputs. Do not route normal fact access through a universal
dynamic serialization value. Serialization belongs at actual boundaries such as configuration,
fixtures, machine-readable output, and snapshots.

Prefer explicit code over metaprogramming. Keep public names aligned with pyinfra so a contributor
can locate the corresponding Python implementation quickly.

## Parallel Execution

Parallelism is a first-class design constraint. It must be considered when translating state,
connectors, facts, operations, output, cancellation, and error handling—not added as a wrapper at
the end.

Preserve pyinfra's execution guarantees:

- Setup, connect, prepare, execute, and disconnect remain distinct stages.
- Preparation runs deploy logic per host and discovers operation ordering without making mutating
  changes.
- Execution re-evaluates each operation against current host state.
- The default strategy is lock-step across hosts: all applicable hosts finish operation N before
  operation N+1 starts.
- Serial, per-host/no-wait, run-once, and per-operation parallel limits retain their pyinfra
  meanings.
- Commands for a single host and operation retain their defined order.
- Connection, fact, preparation, execution, output, and disconnection failures remain attributable
  to the correct host and operation.

Use bounded concurrency. Do not solve concurrency by placing the entire state behind one global
mutex. Prefer host-owned state and explicit coordination. Cancellation, retries, timeouts, output
streaming, failure thresholds, and deterministic reporting must work under concurrency.

Parallelism may improve implementation and throughput, but it must not introduce observable
reordering where pyinfra promises a barrier or dependency order.

## SSH Requirements

SSH is a feature-parity area, not a reduced initial substitute. The Rust connector must ultimately
replicate pyinfra's SSH behavior, including:

- A persistent connection per host with independent command channels.
- Concurrent connection and disconnection across hosts.
- SSH config files and `Include` handling.
- Host aliases, users, ports, identity files, certificates, and identity agents.
- Password, key, and agent authentication.
- Host-key files and strict-host-key policies, including accept-new behavior.
- `ProxyCommand`, single-hop and multi-hop `ProxyJump`, keepalives, and connect timeouts.
- Agent forwarding, PTYs, stdin, command timeouts, exit-status handling, and streamed stdout/stderr.
- SFTP and SCP transfers, including privileged temporary-file handoff and cleanup.
- Connection and transfer retries.
- Rsync through the local rsync/OpenSSH tools where pyinfra does so.

Use established Rust SSH and cryptography crates rather than implementing protocols or primitives.
Crate use does not justify dropping pyinfra behavior. If the Rust ecosystem has a critical gap,
document the gap and agree on the deviation before proceeding.

## Dependencies

Use `clap` for the CLI.

Before writing infrastructure, check in this order:

1. Rust standard library and language facilities.
2. Existing project code.
3. A focused, mature crate.

Use crates for established concerns such as SSH, serialization, templates, graph algorithms,
terminal output, error support, and async/runtime facilities. Evaluate maintenance, security,
platform support, and whether the crate can preserve pyinfra behavior. Do not add a crate merely to
avoid a small amount of straightforward code.

## Reference and Testing

This is a required source-first workflow. Do not design or implement the Rust behavior until the
corresponding pyinfra source and focused behavioral tests or fixtures have been inspected.

For each translated feature:

1. Locate the corresponding pyinfra implementation.
2. Read its focused tests, fixtures, and relevant documentation.
3. Record subtle behavior before translating it.
4. Implement the smallest clear Rust equivalent that preserves that behavior.
5. Add runnable tests at the same behavioral boundary.
6. Compare results with pyinfra where practical.

Pyinfra's JSON/YAML operation and fact fixtures are valuable behavioral specifications. Reuse their
shapes or port cases rather than inventing unrelated expectations. The `pyinfra/` symlink is locally
excluded from Git, so committed tests must not depend on it being present. Preserve MIT attribution
for any substantial copied source or fixtures.

Test observable behavior, especially:

- Generated commands and shell quoting.
- Fact commands, parsing, defaults, and error behavior.
- Operation idempotence and no-change behavior.
- Per-host operation ordering and cycle detection.
- Execution barriers and concurrency limits.
- Retries, timeouts, cancellation, and partial failures.
- SSH configuration, authentication, transfers, and proxy behavior.
- CLI compatibility and exit statuses.

## Project Conventions

- Keep production Rust files at or below 400 lines; split around real responsibilities.
- Never use `unwrap`, `expect`, `panic`, `todo`, or `unimplemented` in production paths.
- Preserve useful error context and distinguish expected host failures from internal defects.
- Quote every user-controlled shell value through the command-building boundary.
- Keep CLI entry points thin and domain behavior in the library.
- Do not duplicate product-owned paths, configuration keys, or environment-variable names across
  modules when a canonical constant is appropriate.
- Do not mix unrelated refactors into a translation change.

Run before completing a change:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc
python scripts/parity_report.py --check
```

Review the final diff for accidental behavior changes, missing tests, stale documentation, dead
code, and unnecessary differences from pyinfra.

## Open Decisions

Deploys are authored as typed Rust code and compiled against rustinfra. Do not add a Python runtime,
Python compatibility frontend, or a separate declarative deploy language.

Other product boundaries remain unsettled, including connector extensibility. Do not decide these
implicitly while implementing another feature. Present the relevant pyinfra behavior, Rust
constraints, and smallest viable choices for human agreement.
