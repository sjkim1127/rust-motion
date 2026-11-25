#!/usr/bin/env python3
"""Prepare a release: bump every version string and draft the changelog entry.

    python3 scripts/release/prepare.py 1.2.0            # edit the working tree
    python3 scripts/release/prepare.py 1.2.0 --dry-run  # only show the diff

It replaces the manual checklist of "grep for the old version" with one command, and checks that
no old version is left behind. It never commits, tags or pushes: review the diff, edit the
generated CHANGELOG draft, open the release PR, and tag after it merges (see docs/RELEASING.md).
"""

from __future__ import annotations

import argparse
import datetime
import difflib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")

# Files where an occurrence of the old version is a release version. Matches that continue into
# more digits (for example the "1.1.0.0" CinemaDNG spec version) are never touched.
PLAIN_FILES = [
    "crates/python/pyproject.toml",
    "crates/ts/package.json",
    "docs/python.md",
    "README.md",
]
HOMEBREW = "packaging/homebrew/rust-motion.rb"
# Skipped when looking for leftovers: history, generated files, and the lockfile (handled by cargo).
LEFTOVER_IGNORE = ("CHANGELOG.md", "Cargo.lock")

SECTIONS = [
    ("breaking", "Breaking changes"),
    ("feat", "Added"),
    ("fix", "Fixed"),
    ("perf", "Performance"),
    ("refactor", "Changed"),
    ("docs", "Documentation"),
]
SKIPPED_TYPES = {"chore", "ci", "test", "build", "style"}
CONVENTIONAL = re.compile(r"^(?P<type>\w+)(?:\((?P<scope>[^)]*)\))?(?P<bang>!)?: (?P<text>.+)$")


def version_pattern(version: str) -> re.Pattern[str]:
    return re.compile(r"(?<![\d.])" + re.escape(version) + r"(?!\.?\d)")


def run(*args: str) -> str:
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def current_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r"(?ms)^\[workspace\.package\].*?^version\s*=\s*\"([^\"]+)\"", text)
    if not match:
        sys.exit("error: could not find [workspace.package] version in Cargo.toml")
    return match.group(1)


def bump_cargo_toml(text: str, old: str, new: str) -> str:
    # [workspace.package] version, then `rust-motion-* = { version = "old", path = ... }` entries.
    text, n = re.subn(
        r'(?ms)(^\[workspace\.package\].*?^version\s*=\s*")' + re.escape(old) + '"',
        lambda m: m.group(1) + new + '"',
        text,
        count=1,
    )
    if n != 1:
        sys.exit("error: [workspace.package] version not updated")
    return re.sub(
        r'(?m)^(rust-motion-[a-z0-9-]+\s*=\s*\{\s*version\s*=\s*")' + re.escape(old) + '"',
        lambda m: m.group(1) + new + '"',
        text,
    )


def bump_homebrew(text: str, old: str, new: str) -> str:
    return re.sub(r'(?m)^(\s*version\s+")' + re.escape(old) + '"', lambda m: m.group(1) + new + '"', text)


def changelog_entry(new: str) -> str:
    try:
        last_tag = run("git", "describe", "--tags", "--abbrev=0").strip()
        revision = f"{last_tag}..HEAD"
    except subprocess.CalledProcessError:
        last_tag, revision = "", "HEAD"
    subjects = run("git", "log", "--no-merges", "--format=%s", revision).splitlines()

    grouped: dict[str, list[str]] = {key: [] for key, _ in SECTIONS}
    for subject in subjects:
        match = CONVENTIONAL.match(subject)
        if not match:
            grouped["refactor"].append(subject)  # unprefixed subjects land under "Changed"
            continue
        kind, text = match["type"], match["text"]
        if match["scope"]:
            text = f"{match['scope']}: {text}"
        if match["bang"]:
            grouped["breaking"].append(text)
        elif kind in grouped:
            grouped[kind].append(text)
        elif kind not in SKIPPED_TYPES:
            grouped["refactor"].append(text)

    today = datetime.date.today().isoformat()
    lines = [f"## {new} - {today}", "", f"<!-- Draft from commits since {last_tag or 'the first commit'}; edit before merging. -->", ""]
    for key, title in SECTIONS:
        if grouped[key]:
            lines += [f"### {title}", ""] + [f"- {item}" for item in grouped[key]] + [""]
    if len(lines) == 4:
        lines += ["- No user-facing changes.", ""]
    return "\n".join(lines)


def find_leftovers(old: str) -> list[str]:
    """Lines outside manifests, the lockfile and history that still mention the old version."""
    pattern = version_pattern(old)
    found = []
    for path in run("git", "ls-files").splitlines():
        if path.endswith(LEFTOVER_IGNORE) or path.endswith("Cargo.toml"):
            continue
        try:
            text = (ROOT / path).read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if pattern.search(line):
                found.append(f"{path}:{number}: {line.strip()}")
    return found


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("version", help="new version, e.g. 1.2.0")
    parser.add_argument("--dry-run", action="store_true", help="print the diff instead of writing files")
    parser.add_argument("--no-lock", action="store_true", help="skip `cargo update --workspace`")
    args = parser.parse_args()

    new = args.version.removeprefix("v")
    if not SEMVER.match(new):
        sys.exit(f"error: {args.version!r} is not a semantic version like 1.2.0")
    old = current_version()
    if new == old:
        sys.exit(f"error: the workspace is already at {old}")

    changes: dict[str, tuple[str, str]] = {}

    def edit(path: str, transform) -> None:
        file = ROOT / path
        before = file.read_text(encoding="utf-8")
        after = transform(before)
        if after == before:
            sys.exit(f"error: {path} does not mention {old}; update scripts/release/prepare.py")
        changes[path] = (before, after)

    edit("Cargo.toml", lambda t: bump_cargo_toml(t, old, new))
    # Almost every crate inherits the workspace version; a few pin their own and must follow it.
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        text = manifest.read_text(encoding="utf-8")
        bumped = re.sub(
            r'(?ms)(^\[package\].*?^version\s*=\s*")' + re.escape(old) + '"',
            lambda m: m.group(1) + new + '"',
            text,
            count=1,
        )
        if bumped != text:
            changes[str(manifest.relative_to(ROOT))] = (text, bumped)
    for path in PLAIN_FILES:
        edit(path, lambda t: version_pattern(old).sub(new, t))
    edit(HOMEBREW, lambda t: bump_homebrew(t, old, new))
    edit("CHANGELOG.md", lambda t: t.replace("# Changelog\n\n", f"# Changelog\n\n{changelog_entry(new)}\n", 1))

    if args.dry_run:
        for path, (before, after) in changes.items():
            sys.stdout.writelines(difflib.unified_diff(before.splitlines(True), after.splitlines(True), f"a/{path}", f"b/{path}"))
        print(f"\n(dry run) {old} -> {new}: {len(changes)} files would change", file=sys.stderr)
        return

    for path, (_, after) in changes.items():
        (ROOT / path).write_text(after, encoding="utf-8")
    if not args.no_lock:
        subprocess.run(["cargo", "update", "--workspace"], cwd=ROOT, check=True)

    leftovers = find_leftovers(old)
    print(f"Bumped {old} -> {new} in {len(changes)} files.")
    if leftovers:
        print(f"\nStill mentions {old}; check whether these should change:")
        print("\n".join(f"  {line[:140]}" for line in leftovers[:20]))
    print(
        "\nNext:\n"
        "  1. Edit the CHANGELOG draft and review `git diff`.\n"
        f"  2. Update the sha256 in {HOMEBREW} after the release assets exist.\n"
        f"  3. Open the release PR, merge it, then tag: git tag -a v{new} -m \"Rust Motion v{new}\" && git push origin v{new}\n"
        "  See docs/RELEASING.md."
    )


if __name__ == "__main__":
    main()
