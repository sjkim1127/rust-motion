# Sentry: CI defect events

Rust Motion sends Sentry events from CI only, for defects found on `main`.
Nothing in the published crates, the CLI or the Python wheels sends telemetry.

The reporter is [`scripts/ci/sentry_defects.py`](../scripts/ci/sentry_defects.py),
with tests in [`scripts/ci/test_sentry_defects.py`](../scripts/ci/test_sentry_defects.py).
It is modelled on the Fission CI defect reporter and keeps the same contract
shape, so events from both projects can be triaged the same way.

## When it sends

- Only from `push` runs of `sjkim1127/rust-motion` on `refs/heads/main`.
  Pull requests, forks, tags and every other event are a dry run: the job
  summary lists what was found, nothing is sent.
- Only when the `SENTRY_DSN` secret is set. Without it every run is a dry run.
- At most 50 events per run, one per fingerprint and OS. Crashes and visual
  mismatches are kept first when a run hits the cap.
- Reporting never changes a run's result: the script exits 0 once its
  arguments parse, and a reporter error sends nothing.

## What an event carries

Every field is from a closed set or a pattern-checked identifier. A value that
fails its check is **rejected with its event**, never scrubbed or truncated
into something that passes; the job summary lists each rejection.

| Field | Values |
|---|---|
| `kind` | `test_failure`, `test_crash`, `render_failure`, `visual_snapshot_mismatch` |
| `category` | `regression` (test failure, non-panic render error), `crash` (test binary killed by a signal or access violation; render exit 101 or a signal), `confirmed_semantic_defect` (visual snapshot mismatch) |
| `component` | a package declared by `crates/*/Cargo.toml` in the checkout |
| `stage` | `test`, `e2e`, `render` |
| `invariant` | the test id (`lib::module::test`, `test.<target>::test`, `doc::Item`, a target id for a crashed binary), `visual.<golden stem>` or `render.<golden stem>`; `^[A-Za-z0-9_.:/<>-]{1,200}$`, longer ids get a hash suffix |
| `os` | `linux`, `macos`, `windows` |
| `commit` | the run's 40-hex `GITHUB_SHA` |
| `repro_id` | `run:<run id>/sentry-evidence-<stage>-<os>`, the artifact the defect was read from |
| `run_url` | `https://github.com/sjkim1127/rust-motion/actions/runs/<run id>` |
| `environment` | `ci` |
| `release` | `rust-motion@<first 12 hex of the commit>` |
| `fingerprint` | `[kind, component, test or snapshot id]` |
| `exception` | type = `kind`, value = `<kind> in <component>/<stage>` |

Only `confirmed_semantic_defect` and `crash` are eligible for automated triage,
as in Fission. Visual snapshot mismatches are `confirmed_semantic_defect`:
the CPU rasterizer promises the same pixels for the same frame, so a golden
mismatch on `main` breaks that promise.

## What is never sent

- Panic messages, assertion text, test stdout or stderr. The libtest adapter
  reads only `test NAME ... FAILED` lines, cargo's
  `to rerun pass -p <package> <target>` hint and the signal or exit code of a
  crashed test binary; the `---- NAME stdout ----` sections are skipped.
- Rendered pixels, frames, MAE or SSIM values, or any user media. The visual
  adapter reads only a closed-set status per golden frame, and the frame name
  must be a file committed under `tests/visual_golden/`.
- File paths outside the repository, user names, host names, IP addresses,
  credentials, environment variables or memory contents. Events carry no
  `message`, `extra`, `contexts`, `user`, `request`, `breadcrumbs` or
  `server_name`.

## Evidence the reporter reads

CI uploads evidence artifacts named `sentry-evidence-<stage>-<os>`, where
`<stage>` is `test`, `e2e` or `visual`. The reporter downloads them into one
directory and reads:

| File | Written by | Read |
|---|---|---|
| `cargo-test.log` | `cargo test` output, tee'd | failing test names, rerun hint, crash signal |
| `visual-report.json` | `tests/compare_visual_golden.py --report-json` | `{"version": 1, "results": [{"snapshot", "status"}]}` |
| `render-status.json` | the visual workflow's render loop | `{"version": 1, "renders": [{"snapshot", "exit_code"}]}` |

## Running it locally

```bash
python3 -m unittest discover -s scripts/ci -p 'test_*.py'
```

```bash
python3 scripts/ci/sentry_defects.py --scan path/to/downloaded-artifacts --dry-run --events-out events.jsonl
```

A local run is always a dry run unless it looks like a push to `main` of this
repository and `SENTRY_DSN` is set; `--dry-run` forces one regardless.
