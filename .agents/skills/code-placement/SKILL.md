---
name: code-placement
description: Place or move rustinfra Rust code consistently with the repository's established architecture. Use whenever adding production behavior, introducing modules, moving responsibilities, or reorganizing src/api, src/connectors, src/facts, src/operations, or src/main.rs.
---

# Code Placement

Preserve pyinfra's recognizable product boundaries. Translate behavior into the existing area that
owns it; do not introduce generic layers or extension points without a reference behavior requiring
them.

## Areas and dependency direction

- `src/api/` owns transport-independent public data and command descriptions: inventory, config,
  run state, values, and command types. It must not depend on concrete operations or connectors.
- `src/connectors/` owns target lifecycle, command execution, output capture, and file transfer.
  Put shared Unix command preparation in `unix.rs`; keep transport-specific SSH behavior in
  `connectors/ssh/`. Connectors consume `api` command types and must not contain desired-state logic.
- `src/facts/` owns typed, read-only host queries and their parsers. A fact defines its input,
  output, command, default, and parsing. It may depend on `api` and connector execution through the
  shared fact executor, but never on operations.
- `src/operations/` owns desired-state inputs, fact-based convergence, generated commands, deploy
  preparation, ordering, retries, and scheduling. Product operations may depend on `api`,
  `connectors`, and `facts`. Keep operation entry points free of connector implementation details.
- `src/main.rs` is the CLI boundary. Keep argument parsing and presentation there; delegate deploy
  behavior to library modules.

Dependencies flow `api` → `connectors`/`facts` → `operations`, with `operations` coordinating the
other areas. Avoid reverse dependencies.

## Adding product capabilities

- Mirror pyinfra module names: `operations/<name>.rs` and `facts/<name>.rs`.
- Register public modules in the corresponding `mod.rs` and keep public type names aligned with the
  reference symbols.
- Keep a small module in one file. Split only around a real capability once a file approaches 400
  lines, following `operations/files/`, `operations/server/`, `facts/files/`, and
  `connectors/ssh/`.
- Put shared package convergence in `operations/packages.rs` and shared package result handling in
  `facts/packages.rs`; do not force managers with different semantics through those helpers.
- Keep focused unit tests beside the implementation. Put tests requiring a real connector or
  process boundary under `tests/`.

## Placement checks

Before creating a module, search for an existing pyinfra-aligned module or helper. Do not:

- put fact parsing in an operation;
- put package or service policy in a connector;
- expose connector internals through `api`;
- add a catch-all `utils` module when behavior has a clear owner;
- move code solely to reduce line counts;
- combine unrelated reference modules because their generated shell happens to look similar.

When ownership is ambiguous, inspect the corresponding pyinfra source and tests and choose the
smallest placement that keeps its concept recognizable.
