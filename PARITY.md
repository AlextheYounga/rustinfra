# Parity status

## 50% capability milestone

Rustinfra has reached the initial 50% product-capability milestone. This milestone measures whether
the central pyinfra workflow is usable end to end, rather than comparing source lines or claiming
that half of pyinfra's 186 operations and 154 facts have been translated.

The milestone requires all of the following runnable capabilities:

- Typed Rust inventory, host/group data precedence, configuration, commands, and hidden values.
- Rust-native deploy definitions with per-host preparation, stable operation identity, dependency
  ordering, cycle detection, run-once, serial, and per-operation parallel controls.
- The setup, connect, prepare, execute, and disconnect lifecycle with failure thresholds and cleanup.
- Bounded lock-step, serial, and per-host/no-wait execution.
- Ordered commands, retries, conditional retries, timeouts, ignored errors, partial failures, and
  per-host result accounting.
- Local execution and persistent SSH execution, including OpenSSH configuration, authentication,
  host-key policy, proxy transports, agent forwarding, SFTP/SCP, privileged transfer handoff, and
  rsync.
- Typed facts that execute through connectors and are re-evaluated by desired-state operations.
- Runnable idempotent operations covering core filesystem state and transfer, server state, and
  Debian/RPM package convergence.
- A consumer-facing Rust API demonstrated by `cargo run --example local_deploy`.

These capabilities are covered by the all-target/all-feature test suite, including local end-to-end
convergence and a persistent SSH integration test.

## Implemented operation areas

- `server`: shell, wait, kill, hostname, and group.
- `files`: file, directory, link, put, and get.
- `apt`: package installation, removal, version convergence, updates, and common controls.
- `dnf`: package installation, removal, version convergence, updates, cleaning, and latest packages.

## Remaining work toward full parity

The 50% milestone is not full pyinfra compatibility. Major remaining areas include:

- The remainder of the operation and fact catalog, including users, services, repositories,
  templates, archives, databases, language package managers, firewalls, and system configuration.
- Preparation-time change prediction and richer no-op/change reporting.
- Callbacks/events, progress rendering, machine-readable output, and full CLI behavior.
- Additional connectors such as Docker/Podman, chroot, and external connector extensibility.
- Cancellation and complete callback/output behavior during concurrent partial failures.
- Broader platform fixtures and end-to-end coverage across Linux distributions and BSD systems.

Parity work continues by translating focused pyinfra source and fixtures at the same typed fact and
desired-state operation boundaries already established.
