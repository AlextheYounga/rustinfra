# rustinfra Context

## Purpose

`rustinfra` is a Rust translation of [pyinfra](https://pyinfra.com/), an agentless infrastructure
automation tool that turns deploy code into commands and executes them against local machines, SSH
hosts, containers, and other targets.

The goal is to retain pyinfra's product behavior and proven solutions while gaining Rust's type
safety, explicit error handling, efficient concurrency, and ability to ship a native executable.
First-class parallel execution is an explicit project goal. Apart from that enhancement, rustinfra
is a translation—not a reimagining of pyinfra.

## Reference Implementation

The repository has a locally excluded `pyinfra/` symlink to the upstream source checkout. When this
document was written, that checkout was:

- Branch: `3.x`
- Commit: `cd95d5a3`
- Description: `v3.11.1-9-gcd95d5a3`

That version contains roughly 37,000 lines of production Python, 186 operations, 154 facts, 1,224
operation fixtures, and 475 fact fixtures. These figures show that parity is a sustained porting
effort, not a greenfield approximation.

The reference source, tests, fixtures, and documentation define expected behavior. The symlink is a
development aid and must not become a runtime or CI dependency. Updating the baseline should be a
deliberate decision so behavior is not translated from a moving target accidentally.

Pyinfra is MIT licensed. Retain required copyright and license notices when substantial source or
fixture material is copied.

## Translation Policy

Pyinfra has already made the product decisions. Rustinfra should preserve them.

The default approach is to translate logic one-to-one, including behavior that may initially look
unusual. Before changing it, determine which edge case, compatibility requirement, or execution
guarantee it serves. A cleaner-looking alternative is not sufficient reason to redesign it.

There are three acceptable reasons not to translate literally:

1. Rust provides a substantially simpler native mechanism with the same behavior.
2. A mature crate implements the low-level concern while allowing the same surrounding semantics.
3. A critical Rust or ecosystem gap makes the pyinfra design impractical.

The third case requires an explicit decision. Deviations must not be hidden inside implementation
work.

“Simpler” refers to the implementation, not the supported product. Rustinfra may use fewer lines,
stronger types, ownership, enums, traits, RAII, and structured concurrency while retaining the same
inputs, effects, ordering, errors, and edge cases.

## Core Model to Preserve

Pyinfra models a run using these concepts:

- **Config** supplies execution, privilege, retry, shell, and connector defaults.
- **Inventory** contains hosts, groups, global data, group data, host data, and overrides.
- **Host** binds inventory data, connection state, facts, and per-host operation state.
- **Connector** connects to a target and provides command execution and file transfer.
- **Fact** runs a read-only query and parses its output into host state.
- **Operation** compares facts with desired state and yields only the commands needed to converge.
- **Command** represents shell execution, upload, download, callbacks, or rsync.
- **State** coordinates stages, active and failed hosts, operation ordering, callbacks, and results.
- **Deploy** groups and parameterizes operations.

These concepts should remain recognizable in Rust. Python implementation devices—decorators,
dynamic attributes, context proxies, generators, and runtime introspection—should be translated to
the closest clear Rust mechanism rather than reproduced artificially.

Facts should be Rust types implementing a fact trait with typed arguments and output. Operations
should consume typed fact results. Dynamic serialized values are appropriate at external boundaries,
not as the normal internal fact representation.

## Run Lifecycle

A run has five ordered stages:

1. **Setup** loads configuration, inventory, host/group data, and requested work.
2. **Connect** opens target connections and determines active and failed hosts.
3. **Prepare** evaluates deploy logic per host, gathers read-only facts, detects potential changes,
   and constructs the operation-order graph.
4. **Execute** re-evaluates operations against current host state and executes generated commands.
5. **Disconnect** cleans up target connections and reports final results.

Preparation does not retain a complete command list. It advances an operation far enough to detect
whether it would change the host. During execution, the operation runs again and facts are gathered
again, allowing earlier operations to affect later decisions. In the current pyinfra baseline, fact
results are not generally cached; each request executes its fact.

Per-host operation sequences are combined into a directed acyclic graph. The final order preserves
host dependencies and uses source/call order to break ties. Conflicting host paths can create a
cycle, which must be diagnosed rather than arbitrarily reordered.

## Parallelism

Parallelism is the planned enhancement and a foundational constraint.

Pyinfra already performs concurrent work with gevent, but rustinfra should represent concurrency
explicitly in its architecture. Connections, preparation, fact gathering, command execution, output
collection, and disconnection must all be safe and efficient across many hosts.

The enhancement must preserve pyinfra's scheduling semantics:

- **Default/lock-step:** execute one operation across all applicable hosts concurrently, wait for
  that operation to finish, then begin the next operation.
- **Serial:** run hosts and operations serially as pyinfra defines.
- **Per-host/no-wait:** let each host progress through its own ordered operations independently.
- **Per-operation controls:** preserve serial, run-once, and maximum-parallel host controls.
- **Global bound:** enforce the configured maximum concurrency and relevant system-resource limits.

Parallel execution must also preserve retries, retry delays and predicates, command timeouts,
continue-on-error and ignore-error behavior, fail-percentage checks, host deactivation, callbacks,
progress, and final result accounting.

The coordinator should maintain ordering and global policy without forcing all host activity through
one shared mutable lock. Host-local state should remain local where practical, and completed work
should return structured events or results. Output may arrive concurrently, but it must remain
correctly attributed and machine-readable output must remain valid.

## SSH Compatibility

SSH is pyinfra's default remote connector and requires feature parity.

Pyinfra uses a customized Paramiko client rather than the system `ssh` process for normal command
execution. It keeps a connection per host and opens channels for facts and commands. Its behavior
includes SSH config parsing with includes, aliases, keys and certificates, password and agent
authentication, custom identity agents, known-hosts policies, agent forwarding, `ProxyCommand`,
multi-hop `ProxyJump`, keepalives, timeouts, retries, PTYs, stdin, streamed stdout/stderr, SFTP, SCP,
privileged file handoff, and transfer cleanup. Rsync is launched locally and uses system OpenSSH.

Rustinfra must replicate this functionality. Existing Rust SSH, SFTP, cryptography, and config
crates should provide protocol machinery wherever possible, but choosing a crate must not silently
reduce behavior. Any ecosystem limitation should be documented with the affected pyinfra behavior
before a compromise is accepted.

## CLI and Dependencies

The CLI will use `clap`.

Other dependencies should be selected as their corresponding pyinfra area is translated. Prefer
the Rust standard library or focused, mature crates over custom implementations. Likely crate-backed
areas include concurrency, SSH, serialization, templating, graph ordering, progress display,
terminal formatting, and error derivation. Dependency selection is an implementation choice only
when behavioral parity remains intact.

Keep the CLI as a thin boundary over a reusable Rust library. Preserve pyinfra's command names,
arguments, defaults, exit behavior, and output semantics where the Rust interface supports them.

### External Dependency Candidates

The following crates are candidates, not blanket approvals. Add each dependency when translating
the behavior that needs it, after checking the relevant pyinfra implementation and fixtures. Avoid
adding the entire list preemptively.

Likely foundational dependencies are:

- `clap` for CLI parsing.
- `tokio` for asynchronous networking, processes, timeouts, synchronization, and bounded parallel
  execution. Enable only the features in use rather than `full` by default.
- `tokio-util` for structured cancellation when execution coordination requires it.
- `thiserror` for typed library errors.
- `serde` and `serde_json` for external data boundaries, fixtures, and machine-readable output.
- `indexmap` where insertion order is part of pyinfra's observable behavior.
- `tracing` and `tracing-subscriber` for diagnostics that remain attributable under parallel
  execution.

Crates with close equivalents to existing pyinfra behavior include:

- `minijinja` for Jinja2-compatible templates, including strict undefined handling.
- `shlex` for POSIX shell splitting and quoting, subject to comparison with command fixtures.
- `petgraph` for operation dependency graphs and cycle detection. Rustinfra may still need a stable
  traversal on top to preserve pyinfra's tie-breaking order.
- `serde_yaml_ng` for YAML boundaries and ported fixtures when YAML support is introduced.
- `similar` for text diffs.
- `pep440_rs` and `pep508_rs` for Python package versions, requirements, and environment markers.
- `tempfile` for temporary files used by transfers and tests.
- `indicatif`, `anstream`, and `anstyle` for progress and terminal-aware output. Consider
  `comfy-table` if its wrapping and alignment can match the required result output.

The leading SSH candidate is `russh` on Tokio, with `russh-sftp` for SFTP. This combination provides
asynchronous persistent sessions, independent channels, key/password/agent authentication, OpenSSH
certificates, PTYs, forwarding, keepalives, and streamed channel I/O. `ProxyJump` can be built from
`direct-tcpip` channels, while `ProxyCommand` can provide a bidirectional stream to `russh`.

`russh-config` is a possible SSH configuration parser, but must be tested against pyinfra behavior
before adoption. The comparison must cover `Include`, OpenSSH precedence, wildcard and negated host
matching, token and tilde expansion, repeated identity files, certificates, identity agents,
`ProxyCommand`, multi-hop `ProxyJump`, and strict-host-key policies. Known-host handling, retries,
authentication ordering, and transfer policy remain rustinfra responsibilities even when protocol
crates are used. SCP may be translated over a `russh` session channel; rsync should continue to use
the local rsync/OpenSSH tools as pyinfra does.

Do not substitute `ssh2` or the system `ssh` command for normal SSH execution merely to reduce
implementation work: the former is blocking and the latter changes pyinfra's persistent-session
model. Do not use `bollard` for Docker or Podman unless the reference implementation changes to a
daemon API; pyinfra currently invokes their command-line tools, and preserving that behavior is the
default. Avoid generic configuration frameworks and young Rich-style terminal ports until a current
behavior requires them and compatibility has been demonstrated.

Useful development-only candidates include `assert_cmd` and `predicates` for CLI behavior,
`proptest` for parser and quoting edge cases, `insta` for selected diagnostic snapshots, and
`testcontainers` for optional connector integration environments. Existing data-driven pyinfra
fixtures should generally remain explicit compatibility tests rather than being converted wholesale
to snapshots.

Before selecting an exact `russh` version and cryptography backend, define the project's supported
Rust version and verify build portability. Crate availability alone is not evidence of behavioral
parity.

## Validation Strategy

Translation should proceed by behavioral slices rather than by creating empty versions of every
module. For each slice:

1. Identify the pyinfra implementation and all directly relevant collaborators.
2. Read its unit tests, fixture cases, end-to-end tests, and documentation.
3. Translate the behavior and important failure modes.
4. Verify the Rust result with focused tests.
5. Add integration or end-to-end coverage at the real connector or CLI boundary where needed.

Pyinfra's existing JSON/YAML fixtures are useful compatibility specifications for fact parsing and
operation command generation. Committed Rust tests cannot depend on the untracked symlink, so needed
cases must be ported, generated into committed artifacts, or independently represented with proper
attribution.

The project currently validates Rust code with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The repository also enforces a 400-line maximum for Rust source files and checks for duplicated
state-bearing string literals.

## Non-Goals

- Redesigning infrastructure automation independently of pyinfra.
- Reducing pyinfra's feature set to make an easier Rust product.
- Reproducing Python's dynamic machinery when Rust has a direct typed equivalent.
- Implementing standard protocols, cryptography, parsers, or template engines from scratch when a
  suitable crate exists.
- Optimizing for superficial source similarity at the expense of Rust correctness or safety.
- Adding speculative features that neither pyinfra nor the parallelism goal requires.

## Open Questions

The following decisions remain open and should be resolved explicitly when their implementation is
approached:

- How deploys are authored and loaded in a native Rust system, given that arbitrary Python files
  and arbitrary Rust source have different execution models.
- Which concurrency runtime best preserves the required scheduling and I/O behavior.
- Which SSH crate or combination of crates can meet the compatibility requirements.
- How connector extensibility maps from Python entry points to Rust without introducing an
  unnecessary plugin ABI.
- Which inventory and configuration formats accompany the Rust API.

Until resolved, these questions are constraints to investigate, not invitations to choose a new
product model implicitly.
