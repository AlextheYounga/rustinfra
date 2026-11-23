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
- A fixture is qualified only when its ledger row links to the Rust test that exercises its
  observable behavior. Unqualified fixtures remain visible rather than being inferred from symbol
  presence or broad module coverage.

## Progress snapshot

- **Defensible pyinfra experience parity:** 100% of the measured operation and fact catalog.
- **Measured built-in catalog surface:** 380/380 symbols (100.00%); see `PARITY_REPORT.md`.
- **Rust operation surface:** 186/186 source operations across 55 reference modules.
- **Rust fact surface:** 194/194 fixture-backed facts across 59 reference modules.
- **Explicit fixture qualification:** 89/1700 reference fixtures (5.24%).
- **Strongest area:** deploy runtime, concurrency, local execution, and persistent SSH.
- **Largest remaining gap:** fixture-level and platform qualification beyond catalog coverage.

The experience estimate combines the exact catalog measurement with the qualified runtime and
connector foundation. It is not a claim of strict behavioral parity for every present symbol;
remaining fixture qualification and broader release gates stay explicit below.

## Parity accounting

- [x] Generate and commit the authoritative operation symbol ledger from `cd95d5a3`.
- [x] Generate and commit the authoritative fixture-backed fact symbol ledger from `cd95d5a3`.
- [x] Inventory every reference fixture with an explicit qualified, unqualified, or excluded status.
- [ ] **Partial** Link every applicable fixture to passing Rust test evidence.
- [x] Add a deterministic parity report that calculates coverage from the ledgers.
- [x] Prevent CI from accepting unclassified built-in operations, facts, or fixtures.

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
- [x] `apt` — packages, repositories, deb files, PPAs, holds, updates, upgrades, and modern
  keyring management through the shared GPG implementation.
- [x] `brew` — formula and cask convergence across old/new CLIs, repository updates, upgrades, taps,
  trust state, JSON/legacy package facts, shell-safe values, and all reference fixtures.
- [x] `bsdinit` — service control, OpenBSD and FreeBSD boot enablement, rc.conf enablement for other
  BSDs, shell-safe service names, all three facts, and all reference fixtures.
- [x] `cargo` — package install, removal, upgrades, pinned versions, fact parsing, and all reference
  fixtures.
- [x] `choco` — Chocolatey installation, package convergence and upgrades, both facts, and all
  reference fixtures.
- [ ] **Partial** `crontab` — core operation exists; complete fixture qualification is outstanding.
- [x] `dnf` — packages, keys, repositories, RPM files, updates, module streams, quoting, and
  fixture-backed convergence.
- [ ] **Partial** `docker` — plugin, Compose, exec, prune, image, volume, and network convergence are
  fixture-qualified; container, build, and registry authentication remain.
- [x] `files` — file and directory state, transfers, local-tree sync, rsync, block and line editing,
  templates, archives, links, flags, remote copy/move/rename, URL downloads, and shell-safe values.
- [x] `flatpak` — system/user package installation and removal, optional remotes, package metadata,
  package listings, shell-safe values, and all reference fixtures.
- [x] `gem` — package install, removal, upgrades, pinned versions, fact parsing, and all reference
  fixtures.
- [x] `git` — repositories, bare repositories, worktrees, local/global/system configuration,
  branches, tags, tracking, commit convergence, safe directories, ownership, and shell-safe values.
- [x] `go` — package install, removal, upgrades, pinned versions, binary naming, fact parsing, and
  all reference fixtures.
- [x] `gpg` — local, URL, and keyserver installs, dearmoring, fingerprint-aware convergence,
  binary and armored keyring removal, permissions, validation, and shell-safe values.
- [ ] **Partial** `iptables` — chains and rules exist; complete fixture qualification is outstanding.
- [x] `launchd` — start, stop, restart-by-stop/start, status parsing, shell-safe service names, and
  all reference fixtures.
- [x] `lxd` — idempotent container launch, stop, and deletion using typed container state.
- [x] `mysql` — databases, users, privileges, SQL, dumps, loads, TLS/X509 requirements, resource
  limits, typed current-state facts, secret masking, shell-safe values, and reference fixtures.
- [x] `npm` — global and directory package install, removal, upgrades, pinned versions, fact
  parsing, shell-safe directories, and all reference fixtures.
- [x] `openrc` — service control, per-runlevel enablement, both facts, shell-safe service and
  runlevel values, and all reference fixtures.
- [x] `opkg` — repository updates, package install/removal/upgrades, pinning validation,
  configuration, feeds, architectures, installed and upgradeable package facts, and all reference
  fixtures.
- [x] `pacman` — repository updates, full upgrades, package and group install/removal, both facts,
  shell-safe group lookup, and all reference fixtures.
- [x] `pip` — virtualenv and venv creation/removal, package and requirements convergence, pip/pip3 facts, and reference fixtures.
- [x] `pipx` — PEP 508 package install/removal, upgrades, PATH setup, environment and package facts,
  shell-safe extra arguments, and all reference fixtures.
- [x] `pkg` — FreeBSD and OpenBSD command selection, install/removal, automatic and explicit
  package paths, fact parsing, and all reference fixtures.
- [x] `pkgin` — repository updates, full upgrades, package install/removal/upgrades, fact parsing,
  and all reference fixtures.
- [x] `pnpm` — global and project packages, scoped/pinned packages, development dependencies,
  lockfile-based installation, script execution, all three facts, and all reference fixtures.
- [x] `postgres` — databases, roles, SQL, dumps, loads, typed current-state facts, connection
  options, secret masking, shell-safe values, and reference fixtures.
- [x] `postgresql` — deprecated compatibility operation names delegate to the complete `postgres`
  behavior while retaining their reference non-idempotent metadata.
- [x] `puppet` — agent execution, server and port selection, and shell-safe values.
- [x] `python` — Rust-native local callbacks and explicit execution-time errors.
- [x] `runit` — management links, automatic-start state, runsv readiness, service control,
  alternative service/source directories, both facts, and all reference fixtures.
- [x] `selinux` — booleans, file contexts and mappings, direct and listed port types, typed context
  data, shell-safe values, and all reference fixtures.
- [x] `server` — complete catalog including scripts, mounts, modules, hosts entries, timezone,
  package/service dispatch, reboot/reconnect, users, groups, authorized keys, sysctls, locales,
  security limits, shell, wait, kill, and hostname behavior.
- [x] `snap` — package installation/removal, classic confinement, channel refreshes, both facts, and
  all reference fixtures.
- [x] `ssh` — nested commands, key scanning, SCP uploads/downloads, remote-sudo handoff,
  exact known-host matching, typed file state, and shell-safe host, user, port, and path values.
- [x] `supervisorctl` — single/group/all process convergence, configuration and masked
  authentication options, all non-interactive actions, status/PID facts, and all reference fixtures.
- [x] `systemd` — service convergence, daemon reloads, user and machine scopes, unit suffixes,
  typed status/enabled facts, and shell-safe values.
- [x] `sysvinit` — service control, distribution-specific boot enablement, explicit runlevel links,
  status parsing, shell-safe values, and all reference fixtures.
- [x] `upstart` — service control, idempotent override-file enablement, status parsing, shell-safe
  service names, and all reference fixtures.
- [x] `uv` — package, Python, tool, shell PATH, and virtual-environment convergence; typed facts
  for packages, Python catalogs, directories, tools, and version; PEP 508 compatibility; shell-safe
  values; and all reference fixtures.
- [x] `vzctl` — lifecycle commands, creation conflicts, templates, repeated settings, save behavior,
  metadata, and injection-shaped keys and values match all reference fixtures.
- [x] `xbps` — repository updates, full upgrades, package install/removal, required package-manager
  self-updates, both facts, and all reference fixtures.
- [x] `yum` — package and provider convergence, keys, repositories, RPM files, updates, and
  fixture-backed command behavior.
- [x] `zfs` — datasets, filesystems, volumes, and snapshots with property convergence,
  recursive creation/destruction, sparse volumes, and shell-safe names and values.
- [x] `zypper` — package convergence, global arguments, repositories, RPM files, updates, and
  fixture-backed command behavior.

## Fact modules

- [x] `apk`
- [x] `apt`
- [x] `brew`
- [x] `bsdinit`
- [x] `cargo`
- [x] `choco`
- [ ] **Partial** `crontab`
- [x] `deb`
- [x] `dnf`
- [x] `docker` — all inspect, derived container/image/storage, command-specific, version, and
  registry-authentication fixtures are covered by focused parser and command tests.
- [x] `efibootmgr`
- [x] `files`
- [x] `flatpak`
- [x] `freebsd`
- [x] `gem`
- [x] `git`
- [x] `go`
- [x] `gpg`
- [x] `hardware`
- [x] `iptables`
- [x] `launchd`
- [x] `lxd`
- [x] `mysql`
- [x] `npm`
- [x] `openrc`
- [x] `opkg`
- [x] `pacman`
- [x] `pip`
- [x] `pipx`
- [x] `pkg`
- [x] `pkgin`
- [x] `pnpm`
- [x] `podman`
- [x] `postgresql` — role and database parsing, typed values, custom connections, and masked
  passwords match all reference fact fixtures.
- [x] `rpm`
- [x] `runit`
- [x] `selinux`
- [x] `server`
- [x] `snap`
- [x] `supervisorctl`
- [x] `systemd`
- [x] `sysvinit`
- [x] `upstart`
- [x] `uv`
- [x] `vzctl`
- [x] `xbps`
- [x] `yum`
- [x] `zfs`
- [x] `zypper`

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
- [ ] **Partial** Alpine Linux package and OpenRC service workflows.
- [ ] **Missing** Arch Linux qualification.
- [ ] **Missing** SUSE qualification.
- [ ] **Partial** FreeBSD behavior.
- [ ] **Partial** OpenBSD package and rc.d service workflows.
- [ ] **Partial** Other BSD rc.d behavior represented by reference fixtures.
- [ ] **Missing** macOS behavior.
- [x] Non-systemd init systems represented by the reference catalog: BSD init, launchd, OpenRC,
  runit, SysV init, Upstart, and supervisorctl.

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
- [ ] No known in-scope parity issue remains untested.
- [ ] The generated parity report is 100% for operations, facts, connectors, fixtures, runtime
  semantics, and platform qualification.

## Release claim gate

The project may claim full parity only when every item above is complete and the release evidence is
attached to a specific rustinfra revision. The resulting claim is:

> rustinfra has behavioral and feature parity with pyinfra 3.x at commit `cd95d5a3` for Rust-native
> deploys and built-in capabilities.
