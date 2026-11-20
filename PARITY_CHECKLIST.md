# Pyinfra parity checklist

This is the authoritative work list for reaching behavioral and feature parity with pyinfra 3.x at
commit `cd95d5a3`, for Rust-native deploys and built-in capabilities.

The parity scope excludes Python deploy execution, Python API compatibility, automatic migration of
Python deploys, Python plugin compatibility, and external connector extensibility. Everything else
that is observable through pyinfra's built-in product is in scope.

## How to use this checklist

- `[x]` means the behavior is implemented, qualified against the reference, and has no known
  in-scope deviation.
- `[ ]` means the item is not yet qualified. **Partial** identifies existing Rust behavior that
  still needs catalog or fixture coverage. **Missing** means no meaningful implementation exists.
- Check off a module only after every public operation or fact in that module is represented in the
  symbol ledger and its applicable reference fixtures pass.
- New differences belong in the deviation register. An undocumented difference blocks 100% parity.

## Progress snapshot

- **Estimated strict parity:** 30–35%.
- **Rust operation surface:** approximately 47 operations.
- **Reference operation surface:** approximately 177 fixture-backed operations across 47 modules;
  the source manifest remains to be frozen.
- **Rust fact surface:** approximately 59 facts.
- **Reference fact surface:** approximately 194 fixture-backed facts across 50 modules; the source
  manifest remains to be frozen.
- **Strongest area:** deploy runtime, concurrency, local execution, and persistent SSH.
- **Largest gap:** operation and fact catalog breadth.

These counts are provisional until the generated symbol ledger below exists. Progress percentages
must not be updated from estimates alone.

## Parity accounting

- [ ] **Missing** Generate and commit the authoritative operation symbol ledger from `cd95d5a3`.
- [ ] **Missing** Generate and commit the authoritative fact symbol ledger from `cd95d5a3`.
- [ ] **Missing** Map every reference fixture to implemented, excluded, or blocked status.
- [ ] **Missing** Record every known behavioral deviation with its disposition.
- [ ] **Missing** Add a deterministic parity report that calculates coverage from the ledgers.
- [ ] **Missing** Prevent CI from accepting unclassified built-in operations, facts, or fixtures.

## Runtime and execution semantics

- [x] Typed inventory with host/group data precedence.
- [x] Setup, connect, prepare, execute, and disconnect lifecycle.
- [x] Stable operation identity, dependency ordering, and cycle detection.
- [x] Lock-step, serial, and per-host/no-wait execution strategies.
- [x] Run-once and per-operation parallel limits.
- [x] Ordered commands, retries, conditional retries, and timeouts.
- [x] Ignored errors, partial failures, and per-host result accounting.
- [x] Execution-time fact re-evaluation for desired-state operations.
- [ ] **Partial** Preparation-time change prediction and complete no-op/change reporting.
- [ ] **Partial** Cancellation across connection, facts, operations, transfers, and cleanup.
- [ ] **Missing** Complete callback and event lifecycle.
- [ ] **Missing** Progress rendering and deterministic concurrent output behavior.
- [ ] **Missing** Machine-readable output parity.
- [ ] **Missing** Full CLI behavior and exit-status parity.

## Operation modules

- [x] `apk` — repository updates, full and available upgrades, package install/removal/upgrades,
  pinned versions, fact parsing, and all reference fixtures.
- [ ] **Partial** `apt` — packages, legacy repositories, and deb822 sources exist; keys and remaining
  operations are outstanding.
- [ ] **Missing** `brew`
- [ ] **Missing** `bsdinit`
- [x] `cargo` — package install, removal, upgrades, pinned versions, fact parsing, and all reference
  fixtures.
- [ ] **Missing** `choco`
- [ ] **Partial** `crontab` — core operation exists; complete fixture qualification is outstanding.
- [ ] **Partial** `dnf` — package convergence exists; keys, repositories, and remaining operations
  are outstanding.
- [ ] **Missing** `docker`
- [ ] **Partial** `files` — core state, transfers, templates, line editing, and archives exist;
  remaining file operations are outstanding.
- [ ] **Missing** `flatpak`
- [x] `gem` — package install, removal, upgrades, pinned versions, fact parsing, and all reference
  fixtures.
- [ ] **Partial** `git` — repository convergence exists; bare repositories and configuration remain.
- [x] `go` — package install, removal, upgrades, pinned versions, binary naming, fact parsing, and
  all reference fixtures.
- [ ] **Missing** `gpg`
- [ ] **Partial** `iptables` — chains and rules exist; complete fixture qualification is outstanding.
- [ ] **Missing** `launchd`
- [ ] **Missing** `lxd`
- [ ] **Missing** `mysql`
- [x] `npm` — global and directory package install, removal, upgrades, pinned versions, fact
  parsing, shell-safe directories, and all reference fixtures.
- [ ] **Missing** `openrc`
- [x] `opkg` — repository updates, package install/removal/upgrades, pinning validation,
  configuration, feeds, architectures, installed and upgradeable package facts, and all reference
  fixtures.
- [x] `pacman` — repository updates, full upgrades, package and group install/removal, both facts,
  shell-safe group lookup, and all reference fixtures.
- [ ] **Partial** `pip` — virtual environments and packages exist; aliases and complete fixture
  qualification remain.
- [ ] **Missing** `pipx`
- [x] `pkg` — FreeBSD and OpenBSD command selection, install/removal, automatic and explicit
  package paths, fact parsing, and all reference fixtures.
- [x] `pkgin` — repository updates, full upgrades, package install/removal/upgrades, fact parsing,
  and all reference fixtures.
- [ ] **Missing** `pnpm`
- [ ] **Missing** `postgres`
- [ ] **Missing** `postgresql`
- [ ] **Missing** `puppet`
- [ ] **Missing** `python`
- [ ] **Missing** `runit`
- [ ] **Missing** `selinux`
- [ ] **Partial** `server` — shell, wait, kill, hostname, groups, users, sysctl, locales, and security
  limits exist; the rest of the server catalog remains.
- [ ] **Missing** `snap`
- [ ] **Missing** `ssh`
- [ ] **Missing** `supervisorctl`
- [ ] **Partial** `systemd` — service convergence exists; remaining operations and full fixtures
  remain.
- [ ] **Missing** `sysvinit`
- [ ] **Missing** `upstart`
- [ ] **Missing** `uv`
- [ ] **Missing** `vzctl`
- [x] `xbps` — repository updates, full upgrades, package install/removal, required package-manager
  self-updates, both facts, and all reference fixtures.
- [ ] **Missing** `yum`
- [ ] **Missing** `zfs`
- [ ] **Missing** `zypper`

## Fact modules

- [x] `apk`
- [ ] **Partial** `apt`
- [ ] **Missing** `brew`
- [ ] **Missing** `bsdinit`
- [x] `cargo`
- [ ] **Missing** `choco`
- [ ] **Partial** `crontab`
- [ ] **Partial** `deb`
- [ ] **Missing** `dnf`
- [ ] **Missing** `docker`
- [ ] **Missing** `efibootmgr`
- [ ] **Partial** `files`
- [ ] **Missing** `flatpak`
- [ ] **Missing** `freebsd`
- [x] `gem`
- [ ] **Partial** `git`
- [x] `go`
- [ ] **Missing** `gpg`
- [ ] **Missing** `hardware`
- [ ] **Partial** `iptables`
- [ ] **Missing** `launchd`
- [ ] **Missing** `lxd`
- [ ] **Missing** `mysql`
- [x] `npm`
- [ ] **Missing** `openrc`
- [x] `opkg`
- [x] `pacman`
- [ ] **Partial** `pip`
- [ ] **Missing** `pipx`
- [x] `pkg`
- [x] `pkgin`
- [ ] **Missing** `pnpm`
- [ ] **Missing** `podman`
- [ ] **Missing** `postgres`
- [ ] **Missing** `postgresql`
- [ ] **Partial** `rpm`
- [ ] **Missing** `runit`
- [ ] **Missing** `selinux`
- [ ] **Partial** `server`
- [ ] **Missing** `snap`
- [ ] **Missing** `supervisorctl`
- [ ] **Partial** `systemd`
- [ ] **Missing** `sysvinit`
- [ ] **Missing** `upstart`
- [ ] **Missing** `uv`
- [ ] **Missing** `vzctl`
- [x] `xbps`
- [ ] **Missing** `yum`
- [ ] **Missing** `zfs`
- [ ] **Missing** `zypper`

## Built-in connectors

- [ ] **Partial** Local command execution and transfers exist; complete reference qualification
  remains.
- [ ] **Partial** SSH — persistent sessions, authentication, host keys, proxies, SFTP/SCP, rsync,
  privilege handoff, and timeouts exist; complete reference qualification remains.
- [ ] **Missing** Chroot.
- [ ] **Missing** Docker exec connector.
- [ ] **Missing** Docker-over-SSH connector.
- [ ] **Missing** Terraform inventory connector behavior.
- [ ] **Missing** Vagrant connector.
- [ ] **Missing** Podman connector behavior where provided by the reference product.

## Platform qualification

- [ ] **Partial** Debian and Ubuntu package, service, user, file, and repository workflows.
- [ ] **Partial** Fedora/RHEL-family package, service, user, and file workflows.
- [ ] **Missing** Alpine Linux qualification.
- [ ] **Missing** Arch Linux qualification.
- [ ] **Missing** SUSE qualification.
- [ ] **Partial** FreeBSD behavior.
- [ ] **Missing** OpenBSD behavior.
- [ ] **Missing** Other BSD behavior represented by reference fixtures.
- [ ] **Missing** macOS behavior.
- [ ] **Missing** Non-systemd init systems.

## Verification gates

- [ ] Every applicable reference operation fixture has an equivalent passing Rust test.
- [ ] Every applicable reference fact fixture has an equivalent passing Rust test.
- [x] Local end-to-end deployment qualification passes.
- [x] Persistent SSH end-to-end qualification passes.
- [ ] Every built-in executable connector has an end-to-end qualification test.
- [ ] Multi-host concurrency, cancellation, timeout, and partial-failure scenarios pass.
- [ ] File upload, download, SCP, SFTP, rsync, and privileged handoff scenarios pass.
- [ ] Supported platform matrix passes in CI or a documented release qualification environment.
- [ ] No in-scope parity issue remains open.
- [ ] No undocumented deviation remains.
- [ ] The generated parity report is 100% for operations, facts, connectors, fixtures, runtime
  semantics, and platform qualification.

## Release claim gate

The project may claim full parity only when every item above is complete and the release evidence is
attached to a specific rustinfra revision. The resulting claim is:

> rustinfra has behavioral and feature parity with pyinfra 3.x at commit `cd95d5a3` for Rust-native
> deploys and built-in capabilities.
