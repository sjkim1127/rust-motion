#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Report CI defects on main to Sentry as closed-set events.

Reads what CI already writes -- `cargo test` (libtest) output, the visual
regression report and the snapshot render status -- and turns each failing
test, crashed test binary, failed render and visual snapshot mismatch into one
Sentry event under the contract in docs/sentry.md. Every value sent comes from
a closed set or is a pattern-checked identifier; a value that fails its check
is rejected together with its event, never cleaned up. No panic message,
assertion text, test output, file path outside the repository, rendered pixel
or user media is ever read into an event: the adapters look only at structure
(libtest's `test NAME ... FAILED` lines, cargo's rerun hint, closed-set JSON
fields, golden file names present in the checkout).

Sends only from push runs of sjkim1127/rust-motion on main, and only when
SENTRY_DSN is set; otherwise it is a dry run. Either way it writes a job
summary of what was (or would be) sent, and it always exits 0 once its
arguments parse: reporting never changes a job's result.

Evidence layout (CI downloads the run's evidence artifacts into one directory):
  <dir>/sentry-evidence-<stage>-<os>/cargo-test.log      libtest output
  <dir>/sentry-evidence-<stage>-<os>/visual-report.json  compare_visual_golden.py --report-json
  <dir>/sentry-evidence-<stage>-<os>/render-status.json  one exit code per rendered snapshot

Usage:
  sentry_defects.py --scan DIR [--dry-run] [--events-out FILE]
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
REPOSITORY = "sjkim1127/rust-motion"
CLIENT = "rust-motion-ci-defects/1"
GOLDEN_DIR = "tests/visual_golden"

# ── Contract (closed sets and patterns) ──────────────────────────────────────

# kind -> the categories it may carry.
KINDS = {
    "test_failure": frozenset({"regression"}),
    "test_crash": frozenset({"crash"}),
    "render_failure": frozenset({"crash", "regression"}),
    "visual_snapshot_mismatch": frozenset({"confirmed_semantic_defect"}),
}
STAGES = frozenset({"test", "e2e", "render"})
CATEGORIES = frozenset({"confirmed_semantic_defect", "crash", "regression"})
OSES = frozenset({"linux", "macos", "windows"})

COMPONENT_RE = re.compile(r"^rust-motion(?:-[a-z0-9]+)*$")
# Test paths, `<target>` ids, doctest items and golden stems; never free text.
INVARIANT_RE = re.compile(r"^[A-Za-z0-9_.:/<>-]{1,200}$")
REPRO_RE = re.compile(r"^run:[1-9][0-9]{0,19}/sentry-evidence-[a-z0-9]+-[a-z0-9]+$")
COMMIT_RE = re.compile(r"^[0-9a-f]{40}$")
RUN_ID_RE = re.compile(r"^[1-9][0-9]{0,19}$")
RUN_URL_RE = re.compile(r"^https://github\.com/sjkim1127/rust-motion/actions/runs/[1-9][0-9]{0,19}$")
RELEASE_RE = re.compile(r"^rust-motion@[0-9a-f]{12}$")

TAG_KEYS = ("kind", "component", "stage", "invariant", "category", "os", "commit", "repro_id", "run_url")
DEFECT_KEYS = ("kind", "component", "stage", "invariant", "category", "os", "repro_id")
EVENT_KEYS = frozenset({
    "event_id", "timestamp", "platform", "level", "release", "environment", "fingerprint", "tags",
    "exception",
})

MAX_EVENTS = 50
# Ranking at the cap: what breaks the engine's promises goes first.
CATEGORY_ORDER = ("crash", "confirmed_semantic_defect", "regression")


class Rejected(ValueError):
    """A value failed its contract check; the event carrying it is dropped."""

    def __init__(self, field_name: str, reason: str):
        super().__init__(f"{field_name}: {reason}")
        self.field_name = field_name
        self.reason = reason


def workspace_packages(repo_root: Path = REPO_ROOT) -> frozenset[str]:
    """Package names declared by crates/*/Cargo.toml in the checkout (the component closed set)."""
    names = set()
    for manifest in repo_root.glob("crates/*/Cargo.toml"):
        try:
            text = manifest.read_text(encoding="utf-8")
        except OSError:
            continue
        section = None
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("["):
                section = line
                continue
            match = re.fullmatch(r'name\s*=\s*"([^"]+)"', line)
            if section == "[package]" and match:
                names.add(match.group(1))
                break
    return frozenset(n for n in names if COMPONENT_RE.fullmatch(n))


def golden_snapshots(repo_root: Path = REPO_ROOT) -> frozenset[str]:
    """Golden frame file names committed under tests/visual_golden (the snapshot closed set)."""
    return frozenset(p.name for p in (repo_root / GOLDEN_DIR).glob("*.png") if SNAPSHOT_RE.fullmatch(p.name))


def check_tags(tags: dict, packages: frozenset[str]) -> None:
    """Raise Rejected unless every tag is present and valid under the contract."""
    if set(tags) != set(TAG_KEYS):
        raise Rejected("tags", "tag set differs from the contract")
    _check_fields(tags, TAG_KEYS, packages)


def _check_fields(tags: dict, keys, packages: frozenset[str]) -> None:
    for key in keys:
        if not isinstance(tags.get(key), str):
            raise Rejected(key, "not a string")
    component = tags.get("component", "")
    checks = (
        ("kind", tags.get("kind") in KINDS, "not a contract kind"),
        ("component", bool(COMPONENT_RE.fullmatch(component)) and component in packages,
         "not a workspace package"),
        ("stage", tags.get("stage") in STAGES, "not a contract stage"),
        ("invariant", bool(INVARIANT_RE.fullmatch(tags.get("invariant", ""))) and ".." not in tags.get("invariant", ""),
         "fails ^[A-Za-z0-9_.:/<>-]{1,200}$ or contains '..'"),
        ("category", tags.get("category") in CATEGORIES, "not a contract category"),
        ("os", tags.get("os") in OSES, "not a contract os"),
        ("commit", bool(COMMIT_RE.fullmatch(tags.get("commit", ""))), "not a 40-hex SHA"),
        ("repro_id", bool(REPRO_RE.fullmatch(tags.get("repro_id", ""))), "not a run evidence id"),
        ("run_url", bool(RUN_URL_RE.fullmatch(tags.get("run_url", ""))), "not this repository's run URL"),
    )
    for key, ok, reason in checks:
        if key in keys and not ok:
            raise Rejected(key, reason)
    if tags.get("category") not in KINDS[tags["kind"]]:
        raise Rejected("category", f"not a category of {tags['kind']}")


# ── Run context ──────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class RunContext:
    event_name: str
    repository: str
    sha: str
    run_id: str
    ref: str

    @classmethod
    def from_env(cls, env: dict) -> "RunContext":
        return cls(
            event_name=env.get("GITHUB_EVENT_NAME", ""),
            repository=env.get("GITHUB_REPOSITORY", ""),
            sha=env.get("GITHUB_SHA", ""),
            run_id=env.get("GITHUB_RUN_ID", ""),
            ref=env.get("GITHUB_REF", ""),
        )

    def refusal(self) -> str | None:
        """Why events from this run must not be sent, or None if they may be."""
        if self.event_name != "push":
            return f"event '{self.event_name or 'unknown'}' is not a push"
        if self.repository != REPOSITORY:
            return f"the run is not in {REPOSITORY} (a fork)"
        if self.ref != "refs/heads/main":
            return "the run is not on main"
        if not COMMIT_RE.fullmatch(self.sha):
            return "GITHUB_SHA is not a 40-hex SHA"
        if not RUN_ID_RE.fullmatch(self.run_id):
            return "GITHUB_RUN_ID is not a run id"
        return None

    environment = "ci"

    @property
    def run_url(self) -> str:
        return f"https://github.com/{REPOSITORY}/actions/runs/{self.run_id}"

    @property
    def release(self) -> str:
        return f"rust-motion@{self.sha[:12]}"


# ── Defects and the summary ledger ───────────────────────────────────────────


@dataclass(frozen=True)
class Defect:
    kind: str
    component: str
    stage: str
    # The test or snapshot id: the fingerprint uses it whole, the tag shortens it.
    test_id: str
    category: str
    os: str
    artifact: str
    source: str  # where it was found (evidence file, test id), summary only

    def fingerprint(self) -> list[str]:
        return [self.kind, self.component, self.test_id]


@dataclass
class Ledger:
    """Everything an adapter saw, for the job summary."""

    defects: list[Defect] = field(default_factory=list)
    # (source, reason) for findings that are not defects under the contract.
    not_defects: list[tuple[str, str]] = field(default_factory=list)
    # (source, field, reason) for defects dropped because a value failed its check.
    rejected: list[tuple[str, str, str]] = field(default_factory=list)
    inputs: list[str] = field(default_factory=list)

    def extend(self, other: "Ledger") -> None:
        self.defects += other.defects
        self.not_defects += other.not_defects
        self.rejected += other.rejected
        self.inputs += other.inputs


def invariant_tag(test_id: str) -> str:
    """The test id as a tag value: Sentry caps tags at 200 chars, so longer ids get a hash suffix."""
    if len(test_id) <= 200:
        return test_id
    digest = hashlib.sha256(test_id.encode()).hexdigest()[:8]
    return f"{test_id[:191]}.{digest}"


def tags_for(defect: Defect, ctx: RunContext) -> dict:
    return {
        "kind": defect.kind,
        "component": defect.component,
        "stage": defect.stage,
        "invariant": invariant_tag(defect.test_id),
        "category": defect.category,
        "os": defect.os,
        "commit": ctx.sha,
        "repro_id": f"run:{ctx.run_id}/{defect.artifact}",
        "run_url": ctx.run_url,
    }


def _add(ledger: Ledger, defect: Defect, ctx: RunContext, packages: frozenset[str]) -> None:
    """Validate a defect's own tags now so a bad value is reported against its source."""
    if len(defect.test_id) > 512:
        ledger.rejected.append((defect.source[:80], "invariant", "test id longer than 512 chars"))
        return
    try:
        _check_fields(tags_for(defect, ctx), DEFECT_KEYS, packages)
    except Rejected as err:
        ledger.rejected.append((defect.source, err.field_name, err.reason))
        return
    ledger.defects.append(defect)


def build_event(defect: Defect, ctx: RunContext, packages: frozenset[str], *,
                event_id: str | None = None, timestamp: str | None = None) -> dict:
    """One Sentry event, validated; raises Rejected on any contract violation."""
    tags = tags_for(defect, ctx)
    check_tags(tags, packages)
    if ctx.environment != "ci":
        raise Rejected("environment", "not ci")
    if not RELEASE_RE.fullmatch(ctx.release):
        raise Rejected("release", "not a contract release name")
    event = {
        "event_id": event_id or uuid.uuid4().hex,
        "timestamp": timestamp or _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": "other",
        "level": "error",
        "release": ctx.release,
        "environment": ctx.environment,
        "fingerprint": defect.fingerprint(),
        "tags": tags,
        # Built only from closed-set values that were just checked.
        "exception": {"values": [{
            "type": defect.kind,
            "value": f"{defect.kind} in {defect.component}/{defect.stage}",
        }]},
    }
    assert set(event) == EVENT_KEYS
    return event


def apply_caps(defects: list[Defect], limit: int = MAX_EVENTS) -> tuple[list[Defect], list[Defect], list[Defect]]:
    """(kept, duplicates, over the run cap), deterministically ordered.

    One event per fingerprint and OS: the same test failing on three runners is
    three events of one issue, so its `os` tag shows where it fails.
    """
    order = {c: i for i, c in enumerate(CATEGORY_ORDER)}
    ranked = sorted(defects, key=lambda d: (order.get(d.category, 9), d.fingerprint(), d.os, d.artifact))
    seen: set[tuple[str, ...]] = set()
    unique: list[Defect] = []
    duplicates: list[Defect] = []
    for defect in ranked:
        key = (*defect.fingerprint(), defect.os)
        if key in seen:
            duplicates.append(defect)
        else:
            seen.add(key)
            unique.append(defect)
    return unique[:limit], duplicates, unique[limit:]


# ── Adapters ─────────────────────────────────────────────────────────────────

ANSI_RE = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")


def _clean(line: str) -> str:
    # A tee'd log keeps CARGO_TERM_COLOR=always escapes; `gh run view --log` adds
    # "<job>\t<step>\t<timestamp> " prefixes.
    line = ANSI_RE.sub("", line).rstrip("\r\n")
    if "\t" in line:
        line = line.rsplit("\t", 1)[1]
        line = re.sub(r"^\d{4}-\d\d-\d\dT[0-9:.]+Z ", "", line)
    return line


# libtest and cargo, as printed for a `cargo test --workspace` run:
#      Running unittests src/lib.rs (target/debug/deps/rust_motion_noise-<hash>)
#   test tests::simplex_matches_reference ... FAILED
#   error: test failed, to rerun pass `-p rust-motion-noise --lib`
#   Caused by:
#     process didn't exit successfully: `...` (signal: 6, SIGABRT: process abort signal)
# Only these lines are read; the `---- NAME stdout ----` sections (panic and
# assertion text) are skipped whole.
BOUNDARY_RE = re.compile(r"^\s*(?:Running|Doc-tests)\s")
FAILED_RE = re.compile(r"^test (.+) \.\.\. FAILED$")
RERUN_RE = re.compile(r"^error: (?:test|doctest) failed, to rerun pass `(.*)`$")
RERUN_ARGS_RE = re.compile(
    r"^-p ([A-Za-z0-9_-]{1,64}) (--lib|--doc|--(?:bin|test|bench|example) [A-Za-z0-9_-]{1,64})$"
)
EXIT_RE = re.compile(r"process didn't exit successfully: .*\((signal: [0-9]+|exit code: 0x[0-9a-fA-F]{8})")
TEST_PATH_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*$")
DOCTEST_RE = re.compile(r"^([A-Za-z0-9_./-]{1,200}\.rs) - (.*?) ?\(line [0-9]+\)$")
DOC_ITEM_RE = re.compile(r"^[A-Za-z0-9_:<>]{1,200}$")


def _target_id(flags: str) -> str:
    """`--lib` -> lib, `--test e2e` -> test.e2e, ... (the test binary within its package)."""
    if flags in ("--lib", "--doc"):
        return flags[2:]
    flag, name = flags.split(" ", 1)
    return f"{flag[2:]}.{name}"


def _test_id(target: str, name: str) -> str | None:
    """The libtest test name as `<target>::<path>`, or None if it is not an identifier path."""
    if target == "doc":
        match = DOCTEST_RE.fullmatch(name)
        if not match or ".." in match.group(1):
            return None
        item = match.group(2)
        if not item:  # a crate- or module-level doc example has no item
            return f"doc::{match.group(1)}"
        return f"doc::{item}" if DOC_ITEM_RE.fullmatch(item) else None
    if name.endswith(" - should panic"):
        name = name[: -len(" - should panic")]
    return f"{target}::{name}" if TEST_PATH_RE.fullmatch(name) and len(name) <= 400 else None


def parse_cargo_test(path: Path, ctx: RunContext, artifact: str, stage: str, os_name: str,
                     packages: frozenset[str], base: Path | None = None) -> Ledger:
    source = _source(path, base)
    ledger = Ledger(inputs=[source])
    try:
        lines = [_clean(raw) for raw in path.read_text(encoding="utf-8", errors="replace").splitlines()]
    except OSError:
        ledger.not_defects.append((source, "unreadable test log (infra, not sent)"))
        return ledger

    failed: list[str] = []
    for i, line in enumerate(lines):
        if BOUNDARY_RE.match(line):
            failed = []
            continue
        match = FAILED_RE.fullmatch(line)
        if match:
            failed.append(match.group(1))
            continue
        rerun = RERUN_RE.fullmatch(line)
        if not rerun:
            continue
        args = RERUN_ARGS_RE.fullmatch(rerun.group(1))
        if not args:
            ledger.rejected.append((source, "component", "cargo rerun hint names no `-p <package>` target"))
            failed = []
            continue
        package, target = args.group(1), _target_id(args.group(2))
        # "Caused by:" and the exit line follow the rerun hint directly.
        crashed = None
        for follow in lines[i + 1:i + 4]:
            exit_match = EXIT_RE.search(follow)
            if exit_match:
                crashed = exit_match.group(1)
                break

        def defect(kind: str, category: str, test_id: str, label: str) -> Defect:
            return Defect(kind=kind, component=package, stage=stage, test_id=test_id, category=category,
                          os=os_name, artifact=artifact, source=label)

        for name in failed:
            test_id = _test_id(target, name)
            label = f"{source}: {package} {target}"
            if test_id is None:
                ledger.rejected.append((label, "invariant", "test name is not an identifier path"))
                continue
            _add(ledger, defect("test_failure", "regression", test_id, f"{label} {test_id}"), ctx, packages)
        if crashed:
            _add(ledger, defect("test_crash", "crash", target, f"{source}: {package} {target} ({crashed})"),
                 ctx, packages)
        elif not failed:
            # A target that failed without naming a test (harness = false, or a
            # binary that exited non-zero after its tests).
            _add(ledger, defect("test_failure", "regression", target, f"{source}: {package} {target}"),
                 ctx, packages)
        failed = []
    return ledger


SNAPSHOT_RE = re.compile(r"^([a-z0-9_]{1,64})\.png$")
VISUAL_DEFECTS = {"mismatch", "dimension_mismatch"}
VISUAL_NOT_DEFECTS = {
    "pass": None,
    "missing_actual": "snapshot was not rendered (render-status.json reports the render)",
    "missing_golden": "no golden frame for this snapshot (infra, not sent)",
}


def _read_json(path: Path, ledger: Ledger, source: str, what: str):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        ledger.not_defects.append((source, f"unreadable {what} (infra, not sent)"))
        return None
    if not isinstance(data, dict) or data.get("version") != 1:
        ledger.not_defects.append((source, f"unknown {what} version (not sent)"))
        return None
    return data


def _snapshot_stem(name, goldens: frozenset[str]) -> str | None:
    if not isinstance(name, str):
        return None
    match = SNAPSHOT_RE.fullmatch(name)
    return match.group(1) if match and name in goldens else None


def parse_visual_report(path: Path, ctx: RunContext, artifact: str, os_name: str, packages: frozenset[str],
                        goldens: frozenset[str], base: Path | None = None) -> Ledger:
    """compare_visual_golden.py --report-json: one closed-set status per golden frame."""
    source = _source(path, base)
    ledger = Ledger(inputs=[source])
    report = _read_json(path, ledger, source, "visual report")
    results = report.get("results") if report else None
    if report and not isinstance(results, list):
        ledger.not_defects.append((source, "visual report has no results (not sent)"))
    for result in results if isinstance(results, list) else []:
        if not isinstance(result, dict):
            continue
        status = result.get("status")
        stem = _snapshot_stem(result.get("snapshot"), goldens)
        if status not in VISUAL_DEFECTS:
            reason = VISUAL_NOT_DEFECTS.get(status, "unknown visual status (not sent)")
            if reason:
                ledger.not_defects.append((f"{source} ({stem or '?'})", reason))
            continue
        if stem is None:
            ledger.rejected.append((source, "invariant", "snapshot is not a golden frame in the checkout"))
            continue
        _add(ledger, Defect(
            kind="visual_snapshot_mismatch", component="rust-motion-cli", stage="render",
            test_id=f"visual.{stem}", category="confirmed_semantic_defect", os=os_name,
            artifact=artifact, source=f"{source} ({stem}: {status})",
        ), ctx, packages)
    return ledger


def parse_render_status(path: Path, ctx: RunContext, artifact: str, os_name: str, packages: frozenset[str],
                        goldens: frozenset[str], base: Path | None = None) -> Ledger:
    """One `rust-motion render` exit code per snapshot; 101 is a Rust panic, >128 a signal."""
    source = _source(path, base)
    ledger = Ledger(inputs=[source])
    status = _read_json(path, ledger, source, "render status")
    renders = status.get("renders") if status else None
    for render in renders if isinstance(renders, list) else []:
        if not isinstance(render, dict):
            continue
        code = render.get("exit_code")
        if not isinstance(code, int) or isinstance(code, bool) or not 0 <= code <= 255:
            ledger.rejected.append((source, "category", "exit code is not 0-255"))
            continue
        if code == 0:
            continue
        stem = _snapshot_stem(render.get("snapshot"), goldens)
        if stem is None:
            ledger.rejected.append((source, "invariant", "snapshot is not a golden frame in the checkout"))
            continue
        _add(ledger, Defect(
            kind="render_failure", component="rust-motion-cli", stage="render",
            test_id=f"render.{stem}", category="crash" if code == 101 or code > 128 else "regression",
            os=os_name, artifact=artifact, source=f"{source} ({stem}: exit {code})",
        ), ctx, packages)
    return ledger


# ── Discovery over downloaded artifacts ──────────────────────────────────────

ARTIFACT_RE = re.compile(r"^sentry-evidence-(test|e2e|visual)-(linux|macos|windows)$")


def _source(path: Path, base: Path | None) -> str:
    try:
        return path.relative_to(base).as_posix() if base else path.name
    except ValueError:
        return path.name


def scan(directory: Path, ctx: RunContext, packages: frozenset[str], goldens: frozenset[str]) -> Ledger:
    ledger = Ledger()
    if not directory.is_dir():
        return ledger
    for artifact_dir in sorted(p for p in directory.iterdir() if p.is_dir()):
        artifact = artifact_dir.name
        match = ARTIFACT_RE.fullmatch(artifact)
        if not match:
            ledger.not_defects.append((artifact[:60], "not a sentry-evidence-<stage>-<os> artifact (ignored)"))
            continue
        evidence, os_name = match.groups()
        stage = {"test": "test", "e2e": "e2e", "visual": "render"}[evidence]
        for path in sorted(artifact_dir.rglob("*")):
            if not path.is_file():
                continue
            if path.name == "cargo-test.log":
                ledger.extend(parse_cargo_test(path, ctx, artifact, stage, os_name, packages, directory))
            elif path.name == "visual-report.json":
                ledger.extend(parse_visual_report(path, ctx, artifact, os_name, packages, goldens, directory))
            elif path.name == "render-status.json":
                ledger.extend(parse_render_status(path, ctx, artifact, os_name, packages, goldens, directory))
    return ledger


# ── Sending ──────────────────────────────────────────────────────────────────

DSN_RE = re.compile(r"^https://([0-9a-f]{32})@([A-Za-z0-9.-]+(?::[0-9]{1,5})?)(/[A-Za-z0-9_/-]*)?/([0-9]{1,20})$")


def envelope_target(dsn: str) -> tuple[str, str]:
    """(envelope URL, X-Sentry-Auth header) for a DSN; ValueError if it is not one."""
    match = DSN_RE.fullmatch(dsn.strip())
    if not match:
        raise ValueError("SENTRY_DSN is not an https Sentry DSN")
    key, host, prefix, project = match.groups()
    prefix = (prefix or "").rstrip("/")
    url = f"https://{host}{prefix}/api/{project}/envelope/"
    auth = f"Sentry sentry_version=7, sentry_client={CLIENT}, sentry_key={key}"
    return url, auth


def envelope(event: dict) -> bytes:
    payload = json.dumps(event, separators=(",", ":"), sort_keys=True).encode()
    header = json.dumps({"event_id": event["event_id"], "sent_at": event["timestamp"]}).encode()
    item = json.dumps({"type": "event", "length": len(payload), "content_type": "application/json"}).encode()
    return header + b"\n" + item + b"\n" + payload + b"\n"


def send(events: list[dict], dsn: str, opener=None) -> list[tuple[str, str]]:
    """Send each event; [(event_id, outcome)]. Never raises for network errors."""
    url, auth = envelope_target(dsn)
    opener = opener or urllib.request.urlopen
    outcomes = []
    for i, event in enumerate(events):
        request = urllib.request.Request(url, data=envelope(event), method="POST", headers={
            "Content-Type": "application/x-sentry-envelope",
            "X-Sentry-Auth": auth,
            "User-Agent": CLIENT,
        })
        try:
            with opener(request, timeout=15) as response:
                outcomes.append((event["event_id"], f"sent (HTTP {response.status})"))
        except urllib.error.HTTPError as err:
            outcomes.append((event["event_id"], f"failed (HTTP {err.code})"))
            if err.code == 429:
                outcomes += [(e["event_id"], "not sent (rate limited)") for e in events[i + 1:]]
                break
        except (urllib.error.URLError, OSError) as err:
            outcomes.append((event["event_id"], f"failed ({type(err).__name__})"))
    return outcomes


# ── Summary ──────────────────────────────────────────────────────────────────


def _md(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


def summary(ledger: Ledger, events: list[dict], duplicates: list[Defect], over_cap: list[Defect],
            mode: str, outcomes: dict[str, str]) -> str:
    out = ["## Sentry defect events", "", f"Mode: **{mode}**.", ""]
    out.append(f"Evidence read: {len(ledger.inputs)} file(s)." if ledger.inputs else "No evidence files were found.")
    out.append("")
    verb = "Sent" if mode == "send" else "Would send"
    out.append(f"### {verb}: {len(events)} event(s)")
    if events:
        out += ["", "| kind | component | stage | os | invariant | category | outcome |",
                "|---|---|---|---|---|---|---|"]
        for event in events:
            t = event["tags"]
            out.append("| " + " | ".join(_md(x) for x in (
                t["kind"], t["component"], t["stage"], t["os"], t["invariant"], t["category"],
                outcomes.get(event["event_id"], "dry run"),
            )) + " |")
    for title, items in (("Same fingerprint and OS as an event above (not sent)", duplicates),
                         (f"Over the {MAX_EVENTS}-event run cap (not sent)", over_cap)):
        if items:
            out += ["", f"### {title}: {len(items)}", ""]
            out += [f"- `{d.kind}` `{d.component}` `{_md(d.test_id)}` ({d.os})" for d in items]
    if ledger.not_defects:
        out += ["", f"### Not defects under the contract (not sent): {len(ledger.not_defects)}", ""]
        out += [f"- {_md(src)}: {_md(reason)}" for src, reason in ledger.not_defects]
    if ledger.rejected:
        out += ["", f"### Rejected by validation (not sent): {len(ledger.rejected)}", ""]
        out += [f"- {_md(src)}: `{field_name}` {_md(reason)}" for src, field_name, reason in ledger.rejected]
    return "\n".join(out) + "\n"


def run(args: argparse.Namespace, env: dict, opener=None, repo_root: Path = REPO_ROOT) -> tuple[str, list[dict]]:
    ctx = RunContext.from_env(env)
    packages = workspace_packages(repo_root)
    goldens = golden_snapshots(repo_root)
    ledger = Ledger()
    for directory in args.scan:
        ledger.extend(scan(Path(directory), ctx, packages, goldens))

    refusal = ctx.refusal()
    dsn = env.get("SENTRY_DSN", "").strip()
    if refusal:
        mode = f"dry run: not sending, {refusal}"
    elif args.dry_run:
        mode = "dry run (--dry-run)"
    elif not dsn:
        mode = "dry run: SENTRY_DSN is not set"
    else:
        mode = "send"

    kept, duplicates, over_cap = apply_caps(ledger.defects)
    events = []
    if not refusal:
        for defect in kept:
            try:
                events.append(build_event(defect, ctx, packages))
            except Rejected as err:
                ledger.rejected.append((defect.source, err.field_name, err.reason))
    outcomes: dict[str, str] = {}
    if mode == "send" and events:
        try:
            outcomes = dict(send(events, dsn, opener))
        except ValueError as err:
            mode = f"dry run: {err}"
    if refusal and kept:
        # Show what the run found without building events it may not send.
        ledger.not_defects += [(d.source, f"found {d.kind}; not sent from this run") for d in kept]
    return summary(ledger, events, duplicates, over_cap, mode, outcomes), events


def main(argv: list[str] | None = None, env: dict | None = None, opener=None,
         repo_root: Path = REPO_ROOT) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--scan", action="append", default=[], help="directory of downloaded evidence artifacts")
    parser.add_argument("--dry-run", action="store_true", help="never send, even with SENTRY_DSN set")
    parser.add_argument("--events-out", help="write the (would-be) events as JSON lines here")
    args = parser.parse_args(argv)
    env = dict(os.environ) if env is None else env
    try:
        text, events = run(args, env, opener, repo_root)
    except Exception as err:  # noqa: BLE001 -- reporting must never fail a job
        text, events = f"## Sentry defect events\n\nReporter error: {type(err).__name__}; nothing sent.\n", []
    summary_path = env.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as stream:
            stream.write(text)
    print(text)
    if args.events_out:
        with open(args.events_out, "w", encoding="utf-8") as stream:
            for event in events:
                stream.write(json.dumps(event, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
