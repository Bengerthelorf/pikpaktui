"""JNI v1 folder scans with a scripted local PikPak endpoint (Python 3.11+, JDK 17+)."""
import json
import os
import queue
import select
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from jni_smoke import JAVA_CLIENT

FILES = "/drive/v1/files"
LIBRARY = Path(sys.argv.pop(1)).resolve(strict=True)


def entry(id, parent="", size=None):
    row = dict(id=id, name=id, parent_id=parent, kind="drive#folder" if size is None else "drive#file")
    if size is not None:
        row["size"] = str(size)
    return row


ROOT = [entry("A"), entry("B"), entry("C"), entry("root-file", size=3)]
DRIVE = ROOT + [entry("a-file", "A", 10), entry("b-file", "B", 20)]


class Endpoint(BaseHTTPRequestHandler):
    def handle_call(self):
        url = urlsplit(self.path)
        query = parse_qs(url.query, keep_blank_values=True)
        parent = query.get("parent_id", [None])[0]
        actual = (self.command, url.path, parent)
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        try:
            expected, status, body, gate, filters = self.server.script.get_nowait()
            if expected != actual:
                raise AssertionError(f"expected {expected}, got {actual}")
            if filters is not None:
                actual_filters = json.loads(query["filters"][0])
                if filters != actual_filters:
                    raise AssertionError(f"expected filters {filters}, got {actual_filters}")
            if gate:
                started, release = gate
                started.set()
                if not release.wait(10):
                    raise AssertionError("test did not release the pending request")
        except (AssertionError, queue.Empty) as exc:
            self.server.errors.append(f"unexpected HTTP request {actual}: {exc}")
            status, body = 500, {}
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    do_GET = do_POST = do_PATCH = handle_call

    def log_message(self, *_):
        pass


class FolderScans(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        tmp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(tmp.cleanup)
        cls.work = Path(tmp.name)
        source = cls.work / "NativeClient.java"
        source.write_text(JAVA_CLIENT)
        cls.http = ThreadingHTTPServer(("127.0.0.1", 0), Endpoint)
        cls.addClassCleanup(cls.http.server_close)
        threading.Thread(target=cls.http.serve_forever, daemon=True).start()
        cls.addClassCleanup(cls.http.shutdown)
        base = f"http://127.0.0.1:{cls.http.server_port}"
        cls.jvm = subprocess.Popen(
            ["java", "-XX:-UsePerfData", f"-Djava.io.tmpdir={cls.work}", str(source), str(LIBRARY)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, cwd=cls.work,
            env=dict(os.environ, PIKPAK_AUTH_BASE_URL=base, PIKPAK_DRIVE_BASE_URL=base,
                     NO_PROXY="127.0.0.1", no_proxy="127.0.0.1"))
        cls.addClassCleanup(cls.stop_jvm)

    @classmethod
    def stop_jvm(cls):
        cls.jvm.stdin.close()
        try:
            cls.jvm.wait(timeout=10)
        finally:
            if cls.jvm.poll() is None:
                cls.jvm.kill()
                cls.jvm.wait()
            cls.jvm.stdout.close()

    def setUp(self):
        self.http.script = queue.Queue()
        self.http.errors = []
        self.account(self._testMethodName)

    def tearDown(self):
        self.assertEqual(self.http.errors, [])
        self.assertTrue(self.http.script.empty(), "missing expected HTTP request")

    def line(self, line):
        self.jvm.stdin.write(line + "\n")
        self.jvm.stdin.flush()
        self.assertTrue(select.select([self.jvm.stdout], [], [], 15)[0], f"no JNI reply for {line}")
        return json.loads(self.jvm.stdout.readline())

    def request(self, op, **args):
        return self.line(json.dumps(dict(op=op, **args)))

    def value(self, op, **args):
        reply = self.request(op, **args)
        self.assertNotIn("error", reply)
        return reply["value"]

    def account(self, name, user_id=""):
        path = self.work / (name + ".json")
        path.write_text(json.dumps(dict(access_token=name, refresh_token="refresh",
                                        expires_at_unix=4102444800, user_id=user_id)))
        self.assertTrue(self.value("init", path=str(path)))
        return path

    def expect(self, method, path, parent, body, status=200, gate=None, filters=None):
        self.http.script.put(((method, path, parent), status, body, gate, filters))

    def listing(self, parent, rows, gate=None):
        self.expect("GET", FILES, parent, {"files": rows}, gate=gate)

    def browse(self):
        self.listing("", ROOT)
        self.assertEqual([e["id"] for e in self.value("files")], [e["id"] for e in ROOT])

    def begin(self, key="scan", **hints):
        self.assertIsNone(self.value("folder_stats_begin", scan=key, **hints))

    def size(self, id, expected, key="scan"):
        reply = self.value("folder_stats", id=id, scan=key)
        self.assertEqual(reply, {"bytes": str(expected), "modified_at": None, "is_empty": expected == 0})

    def test_registered_nested_view_syncs_once_and_cache_survives_a_new_process(self):
        session = self.account("indexed-nested", "nested-owner")
        self.begin()
        started, release = threading.Event(), threading.Event()
        self.expect("GET", FILES, "*", {"files": DRIVE, "sync_time": "2026-11-24T01:00:00Z"},
                    gate=(started, release))
        self.line('async nestedA {"op":"folder_stats","scan":"scan","id":"A"}')
        self.assertTrue(started.wait(5))
        self.line('async nestedB {"op":"folder_stats","scan":"scan","id":"B"}')
        release.set()
        self.assertEqual(self.line("join nestedA")["value"]["bytes"], "10")
        self.assertEqual(self.line("join nestedB")["value"]["bytes"], "20")
        self.value("folder_stats_cancel", scan="scan")
        expected = {
            "A": {"bytes": "10", "modified_at": None, "is_empty": False},
            "C": {"bytes": "0", "modified_at": None, "is_empty": True},
            "missing": None,
        }
        request = dict(op="folder_stats_cached", ids=list(expected))
        self.assertEqual(self.value(**request), expected)
        # Use a fresh Java process to eliminate both Rust and Java memory caches.
        payload = "\n".join(map(json.dumps, (dict(op="init", path=str(session)), request))) + "\n"
        base = f"http://127.0.0.1:{self.http.server_port}"
        result = subprocess.run(
            ["java", "-XX:-UsePerfData", f"-Djava.io.tmpdir={self.work}",
             str(self.work / "NativeClient.java"), str(LIBRARY)],
            cwd=self.work, input=payload, capture_output=True, text=True, check=True, timeout=15,
            env=dict(os.environ, PIKPAK_DRIVE_BASE_URL=base, PIKPAK_AUTH_BASE_URL=base,
                     NO_PROXY="127.0.0.1", no_proxy="127.0.0.1"))
        self.assertEqual(list(map(json.loads, result.stdout.splitlines())), [{"value": True}, {"value": expected}])
        # Logout and sign in under a different account at the identical app-private path.
        self.value("logout", path=str(session))
        self.account("indexed-nested", "other-owner")
        self.assertEqual(self.value(**request), dict.fromkeys(expected))

    def test_explicit_sync_refreshes_by_delta_and_full_reconciliation_is_opt_in(self):
        self.account("indexed-refresh", "refresh-owner")
        self.browse()
        self.begin(root=True)
        baseline = {"phase": {"eq": "PHASE_TYPE_COMPLETE"}, "trashed": {"eq": False}}
        self.expect("GET", FILES, "*", {"files": ROOT, "sync_time": "2025-03-04T09:00:00.010+08:00",
                                        "next_page_token": "p2"}, filters=baseline)
        self.expect("GET", FILES, "*", {"files": DRIVE[len(ROOT):],
                                        "sync_time": "2026-11-24T09:00:00.970+08:00"}, filters=baseline)
        self.assertIsNone(self.value("folder_index_sync", scan="scan"))
        self.size("A", 10)
        self.begin("refresh", root=True, fresh=True)
        self.assertEqual(self.request("folder_index_sync", scan="scan")["code"], "cancelled")
        self.expect("GET", FILES, "*", {"files": [dict(id="a-file", trashed=True)],
                    "sync_time": "2026-11-24T09:01:00.020+08:00"},
                    filters={"phase": {"eq": "PHASE_TYPE_COMPLETE"},
                             "modified_time": {"gt": "2026-11-24T08:59:59.970+08:00"}})
        self.size("A", 0, "refresh")
        self.size("B", 20, "refresh")
        self.expect("GET", FILES, "*", {"files": DRIVE, "sync_time": "2026-11-24T01:02:00Z"}, filters=baseline)
        self.assertIsNone(self.value("folder_index_sync", scan="refresh", force_full=True))
        self.size("A", 10, "refresh")

    def test_only_named_404_filter_failure_rebuilds_the_checkpoint(self):
        self.account("cursor-recovery", "cursor-recovery-owner")
        self.begin()
        self.expect("GET", FILES, "*", {"files": DRIVE, "sync_time": "2026-11-24T09:00:00.970+08:00"})
        self.value("folder_index_sync", scan="scan")
        seen = {"phase": {"eq": "PHASE_TYPE_COMPLETE"}, "modified_time": {"gt": "2026-11-24T08:59:59.970+08:00"}}
        self.begin("error")
        self.expect("GET", FILES, "*", {"error": "file_not_found"}, status=404, filters=seen)
        self.assertIn("error", self.request("folder_index_sync", scan="error"))
        self.assertEqual(self.value("folder_stats_cached", ids=["A"])["A"]["bytes"], "10")
        self.begin("recover")
        self.expect("GET", FILES, "*", {"error": "file_invalid_file_filters"}, status=404, filters=seen)
        self.expect("GET", FILES, "*", {"files": DRIVE, "sync_time": "2026-11-24T10:00:00.970+08:00"},
                    filters={"trashed": {"eq": False}, "phase": {"eq": "PHASE_TYPE_COMPLETE"}})
        self.value("folder_index_sync", scan="recover")
        self.size("B", 20, "recover")

    def test_missing_sync_time_falls_back_to_requested_subtrees_once_per_view(self):
        self.account("indexed-unsupported", "unsupported-owner")
        self.browse()
        self.begin(root=True)
        self.listing("*", DRIVE)
        self.listing("A", [entry("a", "A", 19)])
        self.size("A", 19)
        self.listing("B", [])
        self.size("B", 0)
        self.assertEqual(self.value("folder_stats_cached", ids=["A", "B"]), {"A": None, "B": None})

    def test_cancel_during_explicit_sync_preserves_committed_snapshot(self):
        self.account("indexed-cancel", "cancel-owner")
        self.begin()
        self.expect("GET", FILES, "*", {"files": DRIVE, "sync_time": "2026-11-24T01:00:00Z"})
        self.value("folder_index_sync", scan="scan")
        self.begin("delta")
        started, release = threading.Event(), threading.Event()
        self.expect("GET", FILES, "*", {"files": [dict(id="a-file", trashed=True)],
                    "sync_time": "2026-11-24T01:01:00Z"}, gate=(started, release))
        self.line('async indexing {"op":"folder_index_sync","scan":"delta"}')
        self.assertTrue(started.wait(5))
        self.assertEqual(self.value("folder_stats_cached", ids=["A"])["A"]["bytes"], "10")
        self.value("folder_stats_cancel", scan="delta")
        release.set()
        self.assertEqual(self.line("join indexing")["code"], "cancelled")
        self.assertEqual(self.value("folder_stats_cached", ids=["A"])["A"]["bytes"], "10")

    def test_checked_batch_once_and_cross_scan_hits(self):
        self.browse()
        self.begin(root=True)
        started, release = threading.Event(), threading.Event()
        self.listing("*", DRIVE, (started, release))
        self.line('async first {"op":"folder_stats","id":"A","scan":"scan"}')
        self.assertTrue(started.wait(5))
        self.line('async second {"op":"folder_stats","id":"B","scan":"scan"}')
        release.set()
        self.assertEqual(self.line("join first")["value"]["bytes"], "10")
        self.assertEqual(self.line("join second")["value"]["bytes"], "20")
        self.size("C", 0)
        self.value("folder_stats_cancel", scan="scan")
        self.begin("legacy")
        self.size("B", 20, "legacy")
        self.assertEqual(self.value("folder_stats", id="A")["bytes"], "10")
        self.browse()  # ordinary files still goes to the server
        self.begin("again", root=True)
        self.size("B", 20, "again")

    def test_cold_threshold_and_explicit_refresh(self):
        self.browse()
        self.begin()
        self.listing("A", [entry("a", "A", 7)])
        self.size("A", 7)
        self.begin("warm", root=True)  # two of three folders are cold: below the 75% threshold
        self.listing("B", [])
        self.size("B", 0, "warm")
        self.begin("fresh", root=True, fresh=True)
        self.assertEqual(self.request("folder_stats", scan="warm", id="B")["code"], "cancelled")
        self.listing("*", DRIVE)
        self.size("A", 10, "fresh")

    def test_incomplete_batch_falls_back_once_without_publishing(self):
        self.browse()
        self.begin(root=True)
        self.listing("*", [row for row in DRIVE if row["id"] != "root-file"])
        self.listing("A", [entry("x", "A", 29)])
        self.size("A", 29)
        self.listing("B", [])
        self.size("B", 0)

    def test_unsupported_batch_falls_back_for_root_itself(self):
        self.browse()
        self.begin(root=True)
        self.expect("GET", FILES, "*", {}, status=400)
        self.listing("", ROOT)
        for id in ("A", "B", "C"):
            self.listing(id, [])
        self.size("", 3)

    def test_missing_root_witness_and_account_and_mutation_boundaries(self):
        self.begin(root=True)
        self.listing("A", [entry("a", "A", 4)])
        self.size("A", 4)
        self.browse()
        self.begin("before", root=True)
        self.expect("PATCH", FILES + "/A", None, {})
        self.value("rename", id="A", name="renamed")
        self.assertEqual(self.request("folder_stats", scan="before", id="A")["code"], "cancelled")
        self.begin("after", root=True)  # a pre-mutation root cannot authorize a wildcard
        self.listing("A", [])
        self.size("A", 0, "after")
        self.account("other-account")
        self.assertEqual(self.request("folder_stats", scan="after", id="A")["code"], "cancelled")
        self.begin("new", root=True)
        self.listing("A", [entry("b", "A", 22)])
        self.size("A", 22, "new")

    def test_cancel_running_batch_does_not_fallback_or_publish(self):
        self.browse()
        self.begin(root=True)
        started, release = threading.Event(), threading.Event()
        self.listing("*", DRIVE, (started, release))
        self.line('async inFlight {"op":"folder_stats","id":"A","scan":"scan"}')
        self.assertTrue(started.wait(5))
        self.value("folder_stats_cancel", scan="scan")
        self.begin()  # immediate key reuse while the old HTTP request is still in flight
        release.set()
        self.assertEqual(self.line("join inFlight")["code"], "cancelled")
        self.listing("A", [entry("a", "A", 9)])
        self.size("A", 9)

    def test_late_root_listing_cannot_replace_the_newer_visible_root(self):
        started, release = threading.Event(), threading.Event()
        self.listing("", [entry("old")], (started, release))
        self.line('async older {"op":"files"}')
        self.assertTrue(started.wait(5))
        self.browse()
        release.set()
        self.assertEqual(self.line("join older")["value"][0]["id"], "old")
        self.begin(root=True)
        self.listing("unlisted", [])  # unrelated folder queries cannot consume the root batch
        self.size("unlisted", 0)
        self.listing("*", DRIVE)
        self.size("B", 20)

    def test_begin_and_cancel_during_login_do_not_wait_for_network(self):
        started, release = threading.Event(), threading.Event()
        self.expect("POST", "/v1/auth/signin", None,
                    {"access_token":"login", "refresh_token":"ok", "expires_in":3600},
                    gate=(started, release))
        self.line("async login " + json.dumps(dict(op="login", path=str(self.work / "login.json"),
                                                   email="fixture@example.invalid", password="fixture",
                                                   challenge_token="solved")))
        self.assertTrue(started.wait(5))
        self.begin("old", root=True, fresh=True)
        self.begin("cancel")
        self.value("folder_stats_cancel", scan="cancel")
        release.set()
        self.assertIsNone(self.line("join login")["value"])
        self.assertEqual(self.request("folder_stats", scan="old", id="A")["code"], "cancelled")

    def test_files_still_refreshes_rejected_credentials(self):
        self.browse()
        self.expect("GET", FILES, "", {"error":"UNAUTHENTICATED"}, status=401)
        self.expect("POST", "/v1/auth/token", None, {"access_token":"renewed", "expires_in":3600})
        self.browse()
        self.expect("GET", FILES, "", {}, status=500)
        self.assertIn("error", self.request("files"))
        self.begin(root=True)
        self.listing("A", [])
        self.size("A", 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
