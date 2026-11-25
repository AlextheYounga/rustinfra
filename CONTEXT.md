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

The frozen ledgers track 186 operations, 194 fixture-backed facts, 1,224 operation fixtures, and 476
fact fixtures. The fixture-backed fact catalog is not the same measure as counting source classes.
Current presence and qualification counts are generated in [PARITY_REPORT.md](PARITY_REPORT.md).

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

### Current Rust entry points

The lifecycle above is the behavioral model. The current Rust API separates registration from
connected change prediction:

1. `PreparedDeploy::prepare` evaluates the typed deploy closure for each inventory host, registers
   operation inputs/options, and resolves ordering. It does not connect or apply operation commands.
2. `DeployRuntime::execute` connects hosts, enters the prepare stage, and calls the read-only preview.
   `PreparedDeploy::preview` is also available to callers with their own connectors.
3. Preview classifies operations by whether command generation returns work. It is not a command
   snapshot or an approval gate; execution generates commands again using `Phase::Execute` facts.
4. The scheduler applies operations, records host results, and the runtime disconnects. A summary is
   emitted when execution reached the result-collection stage; earlier failures need not have one.

Deploy authors remain responsible for avoiding mutating side effects in their Rust preparation
closures. Rustinfra does not sandbox arbitrary deploy code.

### Reporting boundary

`src/operations/report.rs` owns `DeployReporter`, preview/result/summary types, `TextReporter`, and
`NullReporter`. `report/compose.rs` owns terminal presentation only. Runtime and prepared-runner
hooks emit phases, operation starts, output lines, outcomes, and summaries. Connector output
handlers deliver complete lines while retaining captured output for execution results.

`DeployRuntime` defaults to `ComposeReporter::auto()` on stderr. Interactive terminals use
`indicatif` animation and `console` display-width measurement: text stays left-aligned and short
status rows are padded to a shared right-side indicator column. Width is captured at construction;
long result text is retained and can wrap. Non-terminal stderr or `TERM=dumb` uses plain text.

Lines remain host/operation-attributed under concurrency, but cross-host arrival order is timing
dependent. Phase completion symbols do not encode success or failure. `execute` returns the actual
lifecycle result, including disconnect errors. Reporter hooks are not a complete callback/event
system or a machine-readable output protocol.

Pyinfra preparation advances an operation far enough to detect whether it would change the host,
without retaining a complete command list. The current Rust preview instead generates a temporary
command vector and checks whether it is empty; it does not retain that vector for execution.
During execution, the operation runs again and facts are gathered
again, allowing earlier operations to affect later decisions. In the current pyinfra baseline, fact
results are not generally cached; each request executes its fact.

Per-host operation sequences are combined into a directed acyclic graph. The final order preserves
host dependencies and uses source/call order to break ties. Conflicting host paths can create a
cycle, which must be diagnosed rather than arbitrarily reordered.

## Parallelism

Parallelism is an implemented runtime capability and a foundational constraint.

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

The planned standalone CLI must use `clap`; the dependency is present, but `src/main.rs` is currently
empty. The supported workflow is a Rust application using the library, as demonstrated by
`examples/local_deploy.rs`. CLI flags, inventory-file loading, and exit-status compatibility should
not be inferred from the library surface.

Deploys are authored as typed Rust code and compiled against the `rustinfra` library. Python deploy
loading, an embedded Python runtime, and a separate YAML/TOML deploy language are not part of the
product direction. The pyinfra codebase remains the behavioral reference for deploy preparation and
execution semantics, not a runtime dependency.

Other dependencies should be selected as their corresponding pyinfra area is translated. Prefer
the Rust standard library or focused, mature crates over custom implementations. Likely crate-backed
areas include concurrency, SSH, serialization, templating, graph ordering, progress display,
terminal formatting, and error derivation. Dependency selection is an implementation choice only
when behavioral parity remains intact.

Keep the CLI as a thin boundary over a reusable Rust library. Preserve pyinfra's command names,
arguments, defaults, exit behavior, and output semantics where the Rust interface supports them.

### Selected dependencies

The current implementation uses:

- `tokio` and `tokio-util` for async execution, processes, synchronization, and cancellation tools.
- `russh`, `russh-sftp`, and `ssh2-config` for SSH sessions, SFTP, and OpenSSH configuration parsing.
- `serde`, `serde_json`, and `serde_yaml_ng` at serialization and fixture boundaries; `indexmap` for
  insertion-ordered data.
- `minijinja` for templates, `shlex` for shell values, `similar` for diffs, and `tempfile` for temporary
  files and test environments.
- `pep440_rs` and `pep508_rs` for Python package version/requirement behavior.
- `indicatif` and `console` for animated terminal output and Unicode display-width measurement.
- `thiserror` for typed errors and `tracing`/`tracing-subscriber` for diagnostics.
- `assert_cmd`, `predicates`, `proptest`, and indicatif's in-memory terminal support for tests.

`Cargo.toml` and `Cargo.lock` are authoritative for versions and enabled features. No minimum
supported Rust version is declared yet; the package uses the Rust 2024 edition.

### Dependency selection guidance

Add dependencies only when translating behavior that needs them, after checking the relevant
pyinfra source and fixtures. Prefer existing platform and project capabilities first. For example,
the operation graph already has ordering logic; a graph crate is not required merely because one
was considered during planning. Enable only runtime features that are actually used.

The selected SSH stack is `russh` on Tokio, with `russh-sftp` for SFTP. This combination provides
asynchronous persistent sessions, independent channels, key/password/agent authentication, OpenSSH
certificates, PTYs, forwarding, keepalives, and streamed channel I/O. `ProxyJump` can be built from
`direct-tcpip` channels, while `ProxyCommand` can provide a bidirectional stream to `russh`.

The current SSH configuration parser is `ssh2-config`. Its integration must be qualified against
pyinfra behavior, including `Include`, OpenSSH precedence, wildcard and negated host
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

Existing data-driven pyinfra fixtures should remain explicit compatibility tests rather than being
converted wholesale to snapshots. Additional snapshot or container-test dependencies need a concrete
qualification requirement, not a hypothetical future use.

The SSH versions are now pinned by the lockfile. Supported Rust versions, cryptography-backend
portability, and platform qualification still need explicit release policy. Crate availability
alone is not evidence of behavioral parity.

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
cargo test --doc
python scripts/parity_report.py --check
```

The repository also enforces a 400-line maximum for Rust source files and checks for duplicated
state-bearing string literals.

The report check validates committed symbol/fixture ledgers and named test references; it does not
execute those tests or establish behavioral equivalence itself. There is currently no committed
CI workflow. A passing local suite does not substitute for the broader platform and connector gates
in [PARITY_CHECKLIST.md](PARITY_CHECKLIST.md).

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

- How connector extensibility maps from Python entry points to Rust without introducing an
  unnecessary plugin ABI. External plugin compatibility is excluded from the current parity scope,
  not implicitly designed by the built-in connector work.
- Which inventory and configuration formats accompany the Rust API.
- The standalone CLI interface and machine-readable output contract.
- The minimum supported Rust version and release platform matrix.

Until resolved, these questions are constraints to investigate, not invitations to choose a new
product model implicitly.
