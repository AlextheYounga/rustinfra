# Parity status

## 75% capability milestone

Rustinfra has reached the 75% product-capability milestone. As with the 50% milestone below, this
measures complete deploy workflows rather than the percentage of pyinfra operation and fact names
that have been translated.

In addition to the 50% runtime, concurrency, connector, and package-management foundation, this
milestone adds the capabilities needed for a common application deployment:

- System users and groups can be created, updated, and removed with Linux and FreeBSD command
  behavior, secondary groups, home ownership, UID, shell, comment, system-account, and password
  controls.
- Systemd units can be started, stopped, restarted, reloaded, enabled, disabled, and managed in
  user or machine scopes, including daemon reload and non-service unit types.
- Templates are rendered from structured typed data with strict undefined-variable handling and
  uploaded only when their rendered content or requested metadata differs.
- Individual configuration lines can be appended, replaced, or removed with grep/sed-compatible
  matching, optional backups, extended regular expressions, and execution-time idempotency checks.
- Sysctl values are read into typed scalar/list values, converged at runtime, and optionally
  persisted through line convergence.
- Git repositories can be cloned or updated with branch, tag, depth, force, rebase, remote URL,
  submodule, ownership, and already-up-to-date handling.
- Local or remote tar and zip archives can be extracted with format detection, idempotent `creates`
  checks, temporary upload cleanup, additional options, and recursive ownership.
- Generated in-memory content uses the same local/SSH transfer boundary as file uploads without
  leaking persistent local temporary files.

These additions retain the source-first workflow: focused pyinfra source and fixtures define command
ordering, quoting, parsing, no-op behavior, and platform differences. The all-target/all-feature
suite covers the resulting facts and operations, including local end-to-end convergence and the
persistent SSH integration test.

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

- `server`: shell, wait, kill, hostname, group, user, and sysctl.
- `systemd`: service state, enablement, daemon reload, and user/machine scopes.
- `files`: file, directory, link, put, get, template, line, and unarchive.
- `apt`: package installation, removal, version convergence, updates, and common controls.
- `dnf`: package installation, removal, version convergence, updates, cleaning, and latest packages.
- `git`: repository clone, update, checkout, remote reconciliation, and submodules.

## Remaining work toward full parity

The 75% milestone is not full pyinfra compatibility. Major remaining areas include:

- The remainder of the operation and fact catalog, including non-systemd init systems, package
  repositories, databases, language package managers, firewalls, and additional system settings.
- Preparation-time change prediction and richer no-op/change reporting.
- Callbacks/events, progress rendering, machine-readable output, and full CLI behavior.
- Additional connectors such as Docker/Podman, chroot, and external connector extensibility.
- Cancellation and complete callback/output behavior during concurrent partial failures.
- Broader platform fixtures and end-to-end coverage across Linux distributions and BSD systems.

Parity work continues by translating focused pyinfra source and fixtures at the same typed fact and
desired-state operation boundaries already established.
