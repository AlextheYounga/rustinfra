# Parity status

The actionable work list and strict parity accounting live in
[PARITY_CHECKLIST.md](PARITY_CHECKLIST.md). The percentage milestones below measure product
capability, not full catalog parity.

## Current status

The frozen catalog currently records 186/186 operation symbols and 194/194 fixture-backed fact
symbols as present. Explicit test-evidence links qualify 279/1700 reference fixtures (16.41%).
These measure different things: catalog presence is complete, behavioral qualification is not.
The generated [report](PARITY_REPORT.md) is authoritative for counts; the
[checklist](PARITY_CHECKLIST.md) tracks runtime, connectors, platforms, and release gates.

The runtime now provides a read-only change preview, host-attributed live stdout/stderr, per-host
success/no-change/failure reports, and operation summaries. Interactive stderr uses animated phases
and a shared right-side indicator column; redirected stderr and `TERM=dumb` use plain text. Custom
`DeployReporter` implementations and `NullReporter` remain available. These presentation hooks do
not establish complete callback/event or deterministic cross-host output parity.

The supported entry point is a compiled Rust deploy using the library. `src/main.rs` remains empty;
there is no finished standalone CLI. The built-in runtime constructor supports local and SSH targets.

## Historical capability milestones

The 50%, 75%, and 85% milestones below describe earlier runnable workflow increments. They are not
current percentages of behavioral parity and should not be combined with catalog measurements.

### 85% capability milestone

Rustinfra has reached the 85% product-capability milestone. This remains a measure of complete,
runnable infrastructure workflows rather than a claim that 85% of pyinfra's operation and fact
names have been translated.

In addition to the 75% application-deployment foundation, this milestone adds:

- Named or command-identified crontab entries with user selection, standard and special schedules,
  list-valued fields, safe updates/removal, comments, and execution-time no-op detection.
- Python virtual environments using either `virtualenv` or the standard `venv` module, plus typed
  PEP 508-aware pip package, version, upgrade, removal, and requirements-file handling.
- Legacy APT `.list` repositories and modern deb822 `.sources` files, including multi-URI/suite
  parsing, architecture and `Signed-By` fields, custom filenames, removal, and content convergence.
- IPv4 and IPv6 iptables chains and rules, including policies, insertion/append/removal, negated
  filters, extension arguments, comments, ports, NAT targets, validation, and shell-safe values.
- Locale enablement/disablement and security-limit convergence through the existing typed fact and
  line-editing boundaries.

Validation uses the all-target/all-feature suite, clean-code guards, persistent SSH integration
test, and runnable local deploy example. Test totals change as qualification grows.

### 75% capability milestone

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

### 50% capability milestone

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

## Examples of implemented operation areas

This list describes the original workflow foundation, not the complete current catalog. The
operation and fact module inventory is maintained in [PARITY_CHECKLIST.md](PARITY_CHECKLIST.md).

- `server`: shell, wait, kill, hostname, group, user, sysctl, locale, and security limits.
- `systemd`: service state, enablement, daemon reload, and user/machine scopes.
- `files`: file, directory, link, put, get, template, line, and unarchive.
- `apt`: package installation, removal, version convergence, updates, legacy repositories, and
  modern deb822 sources.
- `dnf`: package installation, removal, version convergence, updates, cleaning, and latest packages.
- `git`: repository clone, update, checkout, remote reconciliation, and submodules.
- `crontab`: named and command-identified scheduled jobs for system users.
- `pip`: virtual environments, packages, versions, upgrades, and requirements files.
- `iptables`: IPv4/IPv6 chains, policies, filter rules, extensions, and NAT targets.

## Remaining work toward full parity

Neither the historical milestones nor complete symbol coverage establish full pyinfra compatibility.
Major remaining areas include:

- Reference-fixture qualification across the now-present operation and fact catalog.
- Complete preparation-time change prediction and no-op/change reporting qualification.
- Complete callbacks/events, machine-readable output, and standalone CLI behavior.
- Deterministic concurrent output guarantees beyond host-attributed, serialized display lines.
- Additional built-in connectors such as Docker/Podman and chroot. External connector extensibility
  remains an open design question outside the current parity scope.
- Cancellation and complete callback/output behavior during concurrent partial failures.
- Broader platform fixtures and end-to-end coverage across Linux distributions and BSD systems.

Parity work continues by translating focused pyinfra source and fixtures at the same typed fact and
desired-state operation boundaries already established.
