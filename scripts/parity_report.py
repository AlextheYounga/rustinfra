#!/usr/bin/env python3
"""Generate the deterministic built-in symbol coverage report."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "PARITY_REPORT.md"
VALID_STATUSES = {"present", "missing"}
VALID_FIXTURE_STATUSES = {"qualified", "unqualified", "excluded"}


@dataclass(frozen=True)
class Catalog:
    name: str
    symbols: dict[str, str]

    @property
    def present(self) -> int:
        return sum(status == "present" for status in self.symbols.values())

    @property
    def total(self) -> int:
        return len(self.symbols)


@dataclass(frozen=True)
class FixtureCatalog:
    name: str
    fixtures: dict[str, tuple[str, str]]

    @property
    def qualified(self) -> int:
        return sum(status == "qualified" for status, _ in self.fixtures.values())

    @property
    def excluded(self) -> int:
        return sum(status == "excluded" for status, _ in self.fixtures.values())

    @property
    def total(self) -> int:
        return len(self.fixtures)


def load_catalog(name: str) -> Catalog:
    path = ROOT / "parity" / f"{name}.tsv"
    lines = path.read_text().splitlines()
    if not lines or lines[0] != "symbol\tstatus":
        raise ValueError(f"{path} must start with 'symbol\\tstatus'")

    symbols: dict[str, str] = {}
    for line_number, line in enumerate(lines[1:], start=2):
        try:
            symbol, status = line.split("\t")
        except ValueError as error:
            raise ValueError(f"{path}:{line_number}: expected two tab-separated fields") from error
        if status not in VALID_STATUSES:
            raise ValueError(f"{path}:{line_number}: invalid status {status!r}")
        if symbol in symbols:
            raise ValueError(f"{path}:{line_number}: duplicate symbol {symbol}")
        symbols[symbol] = status
    return Catalog(name, symbols)


def validate_implementation(catalog: Catalog) -> None:
    source = "\n".join(path.read_text() for path in sorted((ROOT / "src" / catalog.name).rglob("*.rs")))
    mismatches = []
    for symbol, status in catalog.symbols.items():
        detected = f'"{symbol}"' in source
        if detected != (status == "present"):
            mismatches.append(f"{symbol}: ledger={status}, source={'present' if detected else 'missing'}")
    if mismatches:
        raise ValueError(f"stale {catalog.name} ledger:\n  " + "\n  ".join(mismatches))


def load_fixture_catalog(name: str) -> FixtureCatalog:
    path = ROOT / "parity" / f"{name.removesuffix('s')}_fixtures.tsv"
    lines = path.read_text().splitlines()
    if not lines or lines[0] != "fixture\tstatus\tevidence":
        raise ValueError(f"{path} must start with 'fixture\\tstatus\\tevidence'")

    fixtures: dict[str, tuple[str, str]] = {}
    for line_number, line in enumerate(lines[1:], start=2):
        try:
            fixture, status, evidence = line.split("\t")
        except ValueError as error:
            raise ValueError(f"{path}:{line_number}: expected three tab-separated fields") from error
        if status not in VALID_FIXTURE_STATUSES:
            raise ValueError(f"{path}:{line_number}: invalid status {status!r}")
        if fixture in fixtures:
            raise ValueError(f"{path}:{line_number}: duplicate fixture {fixture}")
        if status == "qualified" and not evidence:
            raise ValueError(f"{path}:{line_number}: qualified fixture requires test evidence")
        if status == "excluded" and not evidence:
            raise ValueError(f"{path}:{line_number}: excluded fixture requires a reason")
        if status == "unqualified" and evidence:
            raise ValueError(f"{path}:{line_number}: unqualified fixture cannot claim evidence")
        fixtures[fixture] = (status, evidence)
    return FixtureCatalog(name, fixtures)


def validate_fixture_catalog(catalog: FixtureCatalog) -> None:
    for fixture, (status, evidence) in catalog.fixtures.items():
        if status != "qualified":
            continue
        for reference in evidence.split(","):
            try:
                source_path, test_name = reference.rsplit("::", 1)
            except ValueError as error:
                raise ValueError(f"{fixture}: invalid test evidence {reference!r}") from error
            source = ROOT / source_path
            if not source.is_file():
                raise ValueError(f"{fixture}: evidence file does not exist: {source_path}")
            if f"fn {test_name}(" not in source.read_text():
                raise ValueError(f"{fixture}: evidence test does not exist: {reference}")

    reference_root = ROOT / "pyinfra" / "tests" / catalog.name
    if reference_root.is_dir():
        reference = {
            path.relative_to(reference_root).as_posix()
            for path in reference_root.rglob("*")
            if path.is_file()
        }
        frozen = set(catalog.fixtures)
        if reference != frozen:
            missing = sorted(reference - frozen)
            stale = sorted(frozen - reference)
            details = [*(f"missing: {path}" for path in missing), *(f"stale: {path}" for path in stale)]
            raise ValueError(f"stale {catalog.name} fixture ledger:\n  " + "\n  ".join(details))


def render(
    operations: Catalog,
    facts: Catalog,
    operation_fixtures: FixtureCatalog,
    fact_fixtures: FixtureCatalog,
) -> str:
    present = operations.present + facts.present
    total = operations.total + facts.total
    percentage = present * 100 / total
    return f"""# Built-in parity report

This report measures built-in catalog surface against pyinfra 3.x at commit `cd95d5a3`.
Operations come from the reference source decorators; facts come from the reference fixture catalog.
`present` means rustinfra exposes the corresponding typed operation or fact. Behavioral qualification
remains tracked in `PARITY_CHECKLIST.md`; symbol coverage alone is not a claim of complete parity.

| Catalog | Present | Reference | Coverage |
| --- | ---: | ---: | ---: |
| Operations | {operations.present} | {operations.total} | {operations.present * 100 / operations.total:.2f}% |
| Facts | {facts.present} | {facts.total} | {facts.present * 100 / facts.total:.2f}% |
| **Combined** | **{present}** | **{total}** | **{percentage:.2f}%** |

## Reference fixture qualification

Every frozen reference fixture is classified in `parity/operation_fixtures.tsv` or
`parity/fact_fixtures.tsv`. A fixture is qualified only when its ledger row names the Rust test or
tests that exercise its observable behavior.

| Fixture catalog | Qualified | Excluded | Reference | Qualification |
| --- | ---: | ---: | ---: | ---: |
| Operations | {operation_fixtures.qualified} | {operation_fixtures.excluded} | {operation_fixtures.total} | {operation_fixtures.qualified * 100 / operation_fixtures.total:.2f}% |
| Facts | {fact_fixtures.qualified} | {fact_fixtures.excluded} | {fact_fixtures.total} | {fact_fixtures.qualified * 100 / fact_fixtures.total:.2f}% |
| **Combined** | **{operation_fixtures.qualified + fact_fixtures.qualified}** | **{operation_fixtures.excluded + fact_fixtures.excluded}** | **{operation_fixtures.total + fact_fixtures.total}** | **{(operation_fixtures.qualified + fact_fixtures.qualified) * 100 / (operation_fixtures.total + fact_fixtures.total):.2f}%** |

Generated by `python scripts/parity_report.py`; verify with `python scripts/parity_report.py --check`.
"""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if the ledger or report is stale")
    arguments = parser.parse_args()

    operations = load_catalog("operations")
    facts = load_catalog("facts")
    operation_fixtures = load_fixture_catalog("operations")
    fact_fixtures = load_fixture_catalog("facts")
    validate_implementation(operations)
    validate_implementation(facts)
    validate_fixture_catalog(operation_fixtures)
    validate_fixture_catalog(fact_fixtures)
    rendered = render(operations, facts, operation_fixtures, fact_fixtures)

    if arguments.check:
        if not REPORT.exists() or REPORT.read_text() != rendered:
            raise SystemExit("PARITY_REPORT.md is stale; run python scripts/parity_report.py")
    else:
        REPORT.write_text(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
