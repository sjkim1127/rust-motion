# SPDX-License-Identifier: MIT OR Apache-2.0
"""Tests for sentry_defects.py. No test sends anything: every send goes to a fake opener.

Run: python3 -m unittest discover -s scripts/ci -p 'test_*.py'
"""

import argparse
import contextlib
import io
import json
import shutil
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

import sentry_defects as sd

DATA = Path(__file__).resolve().parent / "testdata" / "sentry_defects"
SHA = "1b3fd546c0a1e2b3d4f5a6b7c8d9e0f1a2b3c4d5"
RUN_ID = "45165283632"
# `.invalid` never resolves (RFC 2606): even a missed fake opener cannot reach a server.
DSN = "https://0123456789abcdef0123456789abcdef@o1.ingest.sentry.invalid/4500000000000001"
PACKAGES = sd.workspace_packages()
GOLDENS = sd.golden_snapshots()


def env_for(event="push", ref="refs/heads/main", repo="sjkim1127/rust-motion", **extra):
    env = {
        "GITHUB_EVENT_NAME": event,
        "GITHUB_REPOSITORY": repo,
        "GITHUB_SHA": SHA,
        "GITHUB_RUN_ID": RUN_ID,
        "GITHUB_REF": ref,
    }
    env.update(extra)
    return env


def ctx_for(**kwargs):
    return sd.RunContext.from_env(env_for(**kwargs))


CTX = ctx_for()


def defect(**overrides):
    fields = dict(kind="visual_snapshot_mismatch", component="rust-motion-cli", stage="render",
                  test_id="visual.hello_world_0030", category="confirmed_semantic_defect", os="linux",
                  artifact="sentry-evidence-visual-linux", source="test")
    fields.update(overrides)
    return sd.Defect(**fields)


def write(directory: Path, name: str, text: str) -> Path:
    path = directory / name
    path.write_text(text, encoding="utf-8")
    return path


class FakeResponse:
    status = 200

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class RecordingOpener:
    def __init__(self, fail_with=None):
        self.requests = []
        self.fail_with = fail_with

    def __call__(self, request, timeout):
        self.requests.append(request)
        if self.fail_with is not None:
            raise self.fail_with
        return FakeResponse()


def parse_envelope(body: bytes):
    header, item, payload, tail = body.split(b"\n")
    assert tail == b""
    item = json.loads(item)
    assert item["length"] == len(payload)
    return json.loads(header), item, json.loads(payload)


def _no_network(*args, **kwargs):
    raise AssertionError("a test reached urllib.request.urlopen")


def setUpModule():
    # Fail closed: any send that is not given a fake opener is a test bug.
    global _NETWORK_GUARD
    _NETWORK_GUARD = mock.patch.object(sd.urllib.request, "urlopen", _no_network)
    _NETWORK_GUARD.start()


def tearDownModule():
    _NETWORK_GUARD.stop()


class ClosedSetTests(unittest.TestCase):
    def test_components_come_from_the_checkout(self):
        self.assertIn("rust-motion-cli", PACKAGES)
        self.assertIn("rust-motion-rasterizer", PACKAGES)
        self.assertTrue(all(sd.COMPONENT_RE.fullmatch(p) for p in PACKAGES))

    def test_goldens_come_from_the_checkout(self):
        self.assertIn("hello_world_0030.png", GOLDENS)

    def test_an_empty_checkout_has_empty_closed_sets(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(sd.workspace_packages(Path(tmp)), frozenset())
            self.assertEqual(sd.golden_snapshots(Path(tmp)), frozenset())


class RunContextTests(unittest.TestCase):
    def test_push_to_main_may_send(self):
        self.assertIsNone(CTX.refusal())
        self.assertEqual(CTX.environment, "ci")
        self.assertEqual(CTX.release, f"rust-motion@{SHA[:12]}")
        self.assertEqual(CTX.run_url, f"https://github.com/sjkim1127/rust-motion/actions/runs/{RUN_ID}")

    def test_other_events_are_refused(self):
        for event in ("pull_request", "pull_request_target", "workflow_dispatch", "schedule",
                      "workflow_run", "issue_comment", ""):
            with self.subTest(event=event):
                self.assertIsNotNone(ctx_for(event=event).refusal())

    def test_forks_are_refused(self):
        self.assertIn("fork", ctx_for(repo="someone/rust-motion").refusal())

    def test_runs_off_main_are_refused(self):
        for ref in ("refs/heads/feature", "refs/tags/v1.1.0", "refs/pull/3/merge", "refs/heads/main2"):
            with self.subTest(ref=ref):
                self.assertIsNotNone(ctx_for(ref=ref).refusal())

    def test_bad_run_identity_is_refused(self):
        self.assertIsNotNone(ctx_for(GITHUB_SHA="abc123").refusal())
        self.assertIsNotNone(ctx_for(GITHUB_SHA=SHA.upper()).refusal())
        self.assertIsNotNone(ctx_for(GITHUB_RUN_ID="12; rm").refusal())
        self.assertIsNotNone(ctx_for(GITHUB_RUN_ID="0").refusal())


class SchemaTests(unittest.TestCase):
    def assertRejected(self, field, **overrides):
        with self.assertRaises(sd.Rejected) as caught:
            sd.build_event(defect(**overrides), CTX, PACKAGES)
        self.assertEqual(caught.exception.field_name, field)

    def test_a_valid_event_has_exactly_the_contract_shape(self):
        event = sd.build_event(defect(), CTX, PACKAGES, event_id="0" * 32, timestamp="2026-11-26T00:00:00Z")
        self.assertEqual(set(event), sd.EVENT_KEYS)
        self.assertEqual(set(event["tags"]), set(sd.TAG_KEYS))
        for absent in ("message", "logentry", "breadcrumbs", "extra", "contexts", "user", "request",
                       "server_name", "modules"):
            self.assertNotIn(absent, event)
        self.assertEqual(event["environment"], "ci")
        self.assertEqual(event["level"], "error")
        self.assertEqual(event["fingerprint"], ["visual_snapshot_mismatch", "rust-motion-cli", "visual.hello_world_0030"])
        self.assertEqual(event["tags"]["commit"], SHA)
        self.assertEqual(event["tags"]["repro_id"], f"run:{RUN_ID}/sentry-evidence-visual-linux")
        self.assertEqual(event["exception"]["values"][0],
                         {"type": "visual_snapshot_mismatch", "value": "visual_snapshot_mismatch in rust-motion-cli/render"})

    def test_closed_sets(self):
        self.assertRejected("kind", kind="snapshot_mismatch")
        self.assertRejected("stage", stage="build")
        self.assertRejected("category", category="flaky")
        self.assertRejected("os", os="freebsd")

    def test_kind_and_category_must_agree(self):
        self.assertRejected("category", category="crash")
        self.assertRejected("category", kind="test_failure", category="confirmed_semantic_defect")
        self.assertRejected("category", kind="test_crash", category="regression")
        for category in ("crash", "regression"):
            sd.build_event(defect(kind="render_failure", category=category), CTX, PACKAGES)

    def test_component_must_be_a_workspace_package(self):
        self.assertRejected("component", component="rust-motion-nonexistent")
        self.assertRejected("component", component="serde")
        self.assertRejected("component", component="rust-motion-cli\n")
        self.assertRejected("component", component="")

    def test_invariant_rejects_free_text_traversal_and_length(self):
        for bad in ("assertion failed: left == right", "lib::a b", "../../etc/passwd", "lib::x\ny",
                    "lib::caf\u00e9", ""):
            with self.subTest(bad=bad):
                self.assertRejected("invariant", test_id=bad)

    def test_long_ids_keep_the_full_fingerprint_and_a_short_tag(self):
        long_id = "lib::" + "::".join(["segment_name"] * 30)
        event = sd.build_event(defect(kind="test_failure", category="regression", stage="test",
                                      component="rust-motion-noise", test_id=long_id), CTX, PACKAGES)
        self.assertEqual(event["fingerprint"][2], long_id)
        tag = event["tags"]["invariant"]
        self.assertEqual(len(tag), 200)
        self.assertEqual(tag, sd.invariant_tag(long_id))
        self.assertNotEqual(sd.invariant_tag(long_id), sd.invariant_tag(long_id + "_other"))

    def test_repro_id_must_be_a_run_evidence_artifact(self):
        for bad in ("visual-failure-logs", "sentry-evidence-visual-linux/../x", "https://evil.example/x",
                    "sentry-evidence-Visual-linux"):
            with self.subTest(bad=bad):
                self.assertRejected("repro_id", artifact=bad)

    def test_bad_commit_and_run_url_are_rejected(self):
        with self.assertRaises(sd.Rejected) as caught:
            sd.build_event(defect(), ctx_for(GITHUB_SHA="abc"), PACKAGES)
        self.assertEqual(caught.exception.field_name, "commit")
        with self.assertRaises(sd.Rejected) as caught:
            sd.build_event(defect(), ctx_for(GITHUB_RUN_ID="x"), PACKAGES)
        self.assertEqual(caught.exception.field_name, "repro_id")

    def test_check_tags_requires_exactly_the_contract_tags(self):
        tags = sd.tags_for(defect(), CTX)
        sd.check_tags(tags, PACKAGES)
        with self.assertRaises(sd.Rejected):
            sd.check_tags({**tags, "message": "x"}, PACKAGES)
        missing = dict(tags)
        del missing["os"]
        with self.assertRaises(sd.Rejected):
            sd.check_tags(missing, PACKAGES)
        with self.assertRaises(sd.Rejected) as caught:
            sd.check_tags({**tags, "stage": 3}, PACKAGES)
        self.assertEqual(caught.exception.field_name, "stage")


class CargoTestLogTests(unittest.TestCase):
    def parse(self, path, stage="test", os_name="linux", artifact="sentry-evidence-test-linux"):
        return sd.parse_cargo_test(path, CTX, artifact, stage, os_name, PACKAGES)

    def test_linux_workspace_run_with_colour_codes(self):
        ledger = self.parse(DATA / "cargo-test-linux.log")
        found = sorted((d.kind, d.component, d.test_id, d.category) for d in ledger.defects)
        self.assertEqual(found, [
            ("test_crash", "rust-motion-noise", "test.fbm_turbulence_tests", "crash"),
            ("test_failure", "rust-motion-noise", "doc::add", "regression"),
            ("test_failure", "rust-motion-noise", "lib::tests::rejects_nan", "regression"),
            ("test_failure", "rust-motion-noise", "lib::tests::simplex_matches_reference", "regression"),
        ])
        self.assertEqual(ledger.rejected, [])
        self.assertTrue(all(d.stage == "test" and d.os == "linux" for d in ledger.defects))

    def test_windows_crlf_log_with_access_violation(self):
        ledger = self.parse(DATA / "cargo-test-windows.log", stage="e2e", os_name="windows",
                            artifact="sentry-evidence-e2e-windows")
        found = sorted((d.kind, d.component, d.test_id) for d in ledger.defects)
        self.assertEqual(found, [
            ("test_crash", "rust-motion-cli", "test.e2e_cli_subprocess"),
            ("test_failure", "rust-motion-rasterizer", "lib::render::tests::cpu_backend_matches_golden_bytes"),
        ])
        self.assertTrue(all(d.stage == "e2e" and d.os == "windows" for d in ledger.defects))

    def test_passing_run_has_no_defects(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp), "cargo-test.log",
                         "     Running unittests src/lib.rs (target/debug/deps/x-1)\n"
                         "test a::b ... ok\n\ntest result: ok. 1 passed; 0 failed\n")
            ledger = self.parse(path)
        self.assertEqual((ledger.defects, ledger.rejected), ([], []))

    def test_failed_lines_without_a_rerun_hint_are_not_sent(self):
        # A cancelled or timed-out job never prints cargo's hint: nothing is attributed.
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp), "cargo-test.log", "test a::b ... FAILED\n")
            self.assertEqual(self.parse(path).defects, [])

    def test_failures_belong_to_their_own_target(self):
        log = ("     Running unittests src/lib.rs (x)\n"
               "test only_in_noise ... FAILED\n"
               "     Running unittests src/lib.rs (y)\n"
               "test in_shapes ... FAILED\n"
               "error: test failed, to rerun pass `-p rust-motion-shapes --lib`\n")
        with tempfile.TemporaryDirectory() as tmp:
            ledger = self.parse(write(Path(tmp), "cargo-test.log", log))
        self.assertEqual([(d.component, d.test_id) for d in ledger.defects],
                         [("rust-motion-shapes", "lib::in_shapes")])

    def test_target_failure_without_a_test_name(self):
        log = "error: test failed, to rerun pass `-p rust-motion-cli --bin rust-motion`\n"
        with tempfile.TemporaryDirectory() as tmp:
            ledger = self.parse(write(Path(tmp), "cargo-test.log", log))
        self.assertEqual([(d.kind, d.test_id) for d in ledger.defects], [("test_failure", "bin.rust-motion")])

    def test_names_and_packages_outside_the_contract_are_rejected(self):
        log = ("test it renders the user's clip /home/me/a.mp4 ... FAILED\n"
               "test ok::name ... FAILED\n"
               "error: test failed, to rerun pass `-p rust-motion-noise --lib`\n"
               "test x ... FAILED\n"
               "error: test failed, to rerun pass `-p some-other-crate --lib`\n"
               "test y ... FAILED\n"
               "error: test failed, to rerun pass `--lib`\n"
               "test ../../src/lib.rs - x (line 1) ... FAILED\n"
               "test src/lib.rs - Foo (with spaces) (line 9) ... FAILED\n"
               "error: doctest failed, to rerun pass `-p rust-motion-noise --doc`\n")
        with tempfile.TemporaryDirectory() as tmp:
            ledger = self.parse(write(Path(tmp), "cargo-test.log", log))
        self.assertEqual([d.test_id for d in ledger.defects], ["lib::ok::name"])
        self.assertEqual(sorted(field for _, field, _ in ledger.rejected),
                         ["component", "component", "invariant", "invariant", "invariant"])

    def test_doctest_ids_drop_the_line_number(self):
        self.assertEqual(sd._test_id("doc", "crates/noise/src/lib.rs - noise2d (line 12)"), "doc::noise2d")
        self.assertEqual(sd._test_id("doc", "src/fbm.rs - fbm::FbmOptions<T>::new (line 3)"),
                         "doc::fbm::FbmOptions<T>::new")
        self.assertEqual(sd._test_id("doc", "src/lib.rs - (line 1)"), "doc::src/lib.rs")
        self.assertEqual(sd._test_id("lib", "a::b - should panic"), "lib::a::b")

    def test_unreadable_log_is_not_a_defect(self):
        ledger = self.parse(DATA / "does-not-exist.log")
        self.assertEqual(ledger.defects, [])
        self.assertEqual(len(ledger.not_defects), 1)


class VisualTests(unittest.TestCase):
    def test_mismatches_are_confirmed_semantic_defects(self):
        ledger = sd.parse_visual_report(DATA / "visual-report.json", CTX, "sentry-evidence-visual-linux",
                                        "linux", PACKAGES, GOLDENS)
        self.assertEqual(sorted((d.kind, d.test_id, d.category, d.stage) for d in ledger.defects), [
            ("visual_snapshot_mismatch", "visual.cyberpunk_grid_0030", "confirmed_semantic_defect", "render"),
            ("visual_snapshot_mismatch", "visual.hello_world_0030", "confirmed_semantic_defect", "render"),
        ])
        self.assertEqual(len(ledger.not_defects), 1)  # missing_actual; pass is silent

    def test_snapshots_outside_the_golden_set_are_rejected(self):
        report = {"version": 1, "results": [
            {"snapshot": "../../../etc/passwd", "status": "mismatch"},
            {"snapshot": "not_a_golden_0000.png", "status": "mismatch"},
            {"snapshot": "Hello_World_0030.png", "status": "mismatch"},
            {"snapshot": ["hello_world_0030.png"], "status": "mismatch"},
            {"snapshot": "hello_world_0030.png", "status": "mismatch", "mae": "free text is ignored"},
        ]}
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp), "visual-report.json", json.dumps(report))
            ledger = sd.parse_visual_report(path, CTX, "sentry-evidence-visual-linux", "linux", PACKAGES, GOLDENS)
        self.assertEqual([d.test_id for d in ledger.defects], ["visual.hello_world_0030"])
        self.assertEqual(len(ledger.rejected), 4)

    def test_unknown_statuses_and_versions_are_not_sent(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp), "visual-report.json", json.dumps(
                {"version": 1, "results": [{"snapshot": "hello_world_0030.png", "status": "blurry"}]}))
            ledger = sd.parse_visual_report(path, CTX, "a", "linux", PACKAGES, GOLDENS)
            self.assertEqual((ledger.defects, len(ledger.not_defects)), ([], 1))
            for text in (json.dumps({"version": 2, "results": []}), "not json", "[]"):
                path = write(Path(tmp), "visual-report.json", text)
                ledger = sd.parse_visual_report(path, CTX, "a", "linux", PACKAGES, GOLDENS)
                self.assertEqual((ledger.defects, len(ledger.not_defects)), ([], 1))

    def test_render_exit_codes(self):
        ledger = sd.parse_render_status(DATA / "render-status.json", CTX, "sentry-evidence-visual-linux",
                                        "linux", PACKAGES, GOLDENS)
        self.assertEqual(sorted((d.kind, d.test_id, d.category) for d in ledger.defects), [
            ("render_failure", "render.cyberpunk_grid_0000", "regression"),
            ("render_failure", "render.shapes_filters_0030", "crash"),
        ])

    def test_bad_render_entries_are_rejected(self):
        status = {"version": 1, "renders": [
            {"snapshot": "hello_world_0000.png", "exit_code": "101"},
            {"snapshot": "hello_world_0000.png", "exit_code": True},
            {"snapshot": "hello_world_0000.png", "exit_code": 300},
            {"snapshot": "/tmp/x.png", "exit_code": 134},
            {"snapshot": "hello_world_0000.png", "exit_code": 134},
        ]}
        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp), "render-status.json", json.dumps(status))
            ledger = sd.parse_render_status(path, CTX, "sentry-evidence-visual-linux", "linux", PACKAGES, GOLDENS)
        self.assertEqual([(d.test_id, d.category) for d in ledger.defects], [("render.hello_world_0000", "crash")])
        self.assertEqual(len(ledger.rejected), 4)


class CapTests(unittest.TestCase):
    def test_run_cap_and_dedupe_by_fingerprint_and_os(self):
        same = [defect(), defect(source="again")]
        other_os = [defect(os="windows")]
        many = [defect(kind="test_failure", category="regression", stage="test", test_id=f"lib::t{i:03}",
                       component="rust-motion-noise") for i in range(60)]
        kept, duplicates, over = sd.apply_caps(same + other_os + many)
        self.assertEqual(len(kept), sd.MAX_EVENTS)
        self.assertEqual(len(duplicates), 1)
        self.assertEqual(len(over), 2 + 60 - sd.MAX_EVENTS)

    def test_crashes_and_mismatches_outrank_regressions_at_the_cap(self):
        regressions = [defect(kind="test_failure", category="regression", stage="test", test_id=f"lib::t{i:03}",
                              component="rust-motion-noise") for i in range(60)]
        crash = defect(kind="test_crash", category="crash", stage="test", test_id="lib", component="rust-motion-noise")
        kept, _, _ = sd.apply_caps(regressions + [defect(), crash])
        self.assertEqual([d.category for d in kept[:2]], ["crash", "confirmed_semantic_defect"])

    def test_order_is_deterministic(self):
        items = [defect(test_id=f"visual.s{i}") for i in range(5)]
        self.assertEqual(sd.apply_caps(items)[0], sd.apply_caps(list(reversed(items)))[0])


class ScanAndSendTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.evidence = self.tmp / "evidence"
        for artifact, files in (
            ("sentry-evidence-test-linux", {"cargo-test.log": "cargo-test-linux.log"}),
            ("sentry-evidence-test-windows", {"cargo-test.log": "cargo-test-windows.log"}),
            ("sentry-evidence-visual-linux", {"visual-report.json": "visual-report.json",
                                              "render-status.json": "render-status.json"}),
        ):
            (self.evidence / artifact).mkdir(parents=True)
            for name, src in files.items():
                shutil.copy(DATA / src, self.evidence / artifact / name)
        (self.evidence / "visual-snapshots-abc").mkdir()
        self.summary = self.tmp / "summary.md"

    def run_main(self, *extra, opener=None, **env_overrides):
        env = env_for(**env_overrides)
        env["GITHUB_STEP_SUMMARY"] = str(self.summary)
        events_out = self.tmp / "events.jsonl"
        with contextlib.redirect_stdout(io.StringIO()):
            sd.main(["--scan", str(self.evidence), "--events-out", str(events_out), *extra], env=env, opener=opener)
        events = [json.loads(line) for line in events_out.read_text().splitlines()]
        return self.summary.read_text(), events

    def test_scan_finds_every_adapter(self):
        ledger = sd.scan(self.evidence, CTX, PACKAGES, GOLDENS)
        self.assertEqual({d.kind for d in ledger.defects},
                         {"test_failure", "test_crash", "visual_snapshot_mismatch", "render_failure"})
        self.assertEqual({d.stage for d in ledger.defects}, {"test", "render"})
        self.assertIn(("visual-snapshots-abc", "not a sentry-evidence-<stage>-<os> artifact (ignored)"),
                      ledger.not_defects)

    def test_dry_run_without_dsn_sends_nothing_and_summarises(self):
        text, events = self.run_main()
        self.assertIn("dry run: SENTRY_DSN is not set", text)
        self.assertEqual(len(events), 10)
        self.assertIn("Would send: 10 event(s)", text)

    def test_pull_request_never_sends_even_with_a_dsn(self):
        opener = RecordingOpener()
        text, events = self.run_main(opener=opener, event="pull_request", SENTRY_DSN=DSN)
        self.assertEqual((opener.requests, events), ([], []))
        self.assertIn("not sending", text)
        self.assertIn("not sent from this run", text)

    def test_fork_never_sends(self):
        opener = RecordingOpener()
        _, events = self.run_main(opener=opener, repo="someone/rust-motion", SENTRY_DSN=DSN)
        self.assertEqual((opener.requests, events), ([], []))

    def test_dry_run_flag_wins_over_a_dsn(self):
        opener = RecordingOpener()
        text, _ = self.run_main("--dry-run", opener=opener, SENTRY_DSN=DSN)
        self.assertEqual(opener.requests, [])
        self.assertIn("dry run (--dry-run)", text)

    def test_send_posts_one_envelope_per_event(self):
        opener = RecordingOpener()
        text, events = self.run_main(opener=opener, SENTRY_DSN=DSN)
        self.assertEqual(len(opener.requests), len(events))
        request = opener.requests[0]
        self.assertEqual(request.full_url, "https://o1.ingest.sentry.invalid/api/4500000000000001/envelope/")
        self.assertIn("sentry_key=0123456789abcdef0123456789abcdef", request.headers["X-sentry-auth"])
        header, item, payload = parse_envelope(request.data)
        self.assertEqual(item["type"], "event")
        self.assertEqual(header["event_id"], payload["event_id"])
        self.assertEqual(set(payload), sd.EVENT_KEYS)
        self.assertIn("sent (HTTP 200)", text)

    def test_network_failure_does_not_fail_the_job(self):
        opener = RecordingOpener(fail_with=urllib.error.URLError("down"))
        text, _ = self.run_main(opener=opener, SENTRY_DSN=DSN)
        self.assertIn("failed (URLError)", text)

    def test_rate_limit_stops_sending(self):
        opener = RecordingOpener(fail_with=urllib.error.HTTPError("u", 429, "slow down", {}, None))
        text, events = self.run_main(opener=opener, SENTRY_DSN=DSN)
        self.assertEqual(len(opener.requests), 1)
        self.assertEqual(text.count("not sent (rate limited)"), len(events) - 1)

    def test_an_invalid_dsn_falls_back_to_a_dry_run(self):
        opener = RecordingOpener()
        text, _ = self.run_main(opener=opener, SENTRY_DSN="http://key@sentry.invalid/1")
        self.assertEqual(opener.requests, [])
        self.assertIn("not an https Sentry DSN", text)

    def test_a_reporter_error_does_not_fail_the_job(self):
        out = io.StringIO()
        with mock.patch.object(sd, "run", side_effect=RuntimeError("boom")), contextlib.redirect_stdout(out):
            self.assertEqual(sd.main(["--scan", str(self.evidence)], env={}), 0)
        self.assertIn("Reporter error: RuntimeError; nothing sent.", out.getvalue())

    def test_empty_evidence_is_a_clean_skip(self):
        shutil.rmtree(self.evidence)
        opener = RecordingOpener()
        text, events = self.run_main(opener=opener, SENTRY_DSN=DSN)
        self.assertEqual((events, opener.requests), ([], []))
        self.assertIn("No evidence files were found.", text)


class DsnTests(unittest.TestCase):
    def test_valid_dsn(self):
        url, auth = sd.envelope_target(DSN)
        self.assertEqual(url, "https://o1.ingest.sentry.invalid/api/4500000000000001/envelope/")
        self.assertIn("sentry_client=rust-motion-ci-defects/1", auth)

    def test_invalid_dsns(self):
        for bad in ("", "http://0123456789abcdef0123456789abcdef@sentry.invalid/1",
                    "https://short@sentry.invalid/1", "https://0123456789abcdef0123456789abcdef@sentry.invalid/x"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                sd.envelope_target(bad)


class NoInputTextTests(unittest.TestCase):
    """The panic messages, paths and user text in the evidence never reach an event."""

    def test_no_event_field_contains_log_text(self):
        evidence = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, evidence)
        for artifact, src in (("sentry-evidence-test-linux", "cargo-test-linux.log"),
                              ("sentry-evidence-e2e-windows", "cargo-test-windows.log")):
            (evidence / artifact).mkdir()
            shutil.copy(DATA / src, evidence / artifact / "cargo-test.log")
        args = argparse.Namespace(scan=[str(evidence)], dry_run=True, events_out=None)
        _, events = sd.run(args, env_for())
        self.assertTrue(events)
        blob = json.dumps(events)
        for secret in ("secret.mp4", "/Users/", "private-clip", "C:\\\\Users", "panicked", "left == right",
                       "/home/runner", "SIGABRT", "0xc0000005", "pixel 12,40"):
            self.assertNotIn(secret, blob)


if __name__ == "__main__":
    unittest.main()
