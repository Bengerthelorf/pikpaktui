"""Exercise the Java ABI and JSON envelopes using a host cdylib and a JDK."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
JAVA_CLIENT = """
package com.snaix.pikpak;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class NativeClient {
    private native String request(String json);
    public static void main(String[] args) throws Exception {
        System.load(args[0]);
        var client = new NativeClient();
        var input = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        var jobs = new java.util.HashMap<String, java.util.concurrent.CompletableFuture<String>>();
        for (String line; (line = input.readLine()) != null; ) {
            if (line.startsWith("async ")) {
                String[] parts = line.split(" ", 3);
                jobs.put(parts[1], java.util.concurrent.CompletableFuture.supplyAsync(
                    () -> client.request(parts[2])));
                System.out.println("null");
            } else if (line.startsWith("join ")) {
                System.out.println(jobs.remove(line.substring(5)).get(10, java.util.concurrent.TimeUnit.SECONDS));
            } else {
                System.out.println(client.request(line));
            }
        }
    }
}
"""


def prepare_java_client(work, library):
    source = work / "com" / "snaix" / "pikpak" / "NativeClient.java"
    source.parent.mkdir(parents=True, exist_ok=True)
    source.write_text(JAVA_CLIENT)
    subprocess.run(["javac", "-J-XX:-UsePerfData", f"-J-Djava.io.tmpdir={work}",
                    "--release", "17", "-d", str(work), str(source)], check=True, cwd=work)
    return ["java", "-XX:-UsePerfData", f"-Djava.io.tmpdir={work}", "--enable-native-access=ALL-UNNAMED",
            "-cp", str(work), "com.snaix.pikpak.NativeClient", str(library)]


def smoke(library, work):
    java = prepare_java_client(work, library)
    session = work / "旧会话.json"
    session.write_text(json.dumps(dict(access_token="test", refresh_token="old", expires_at_unix=0)))
    upload = work / "📁.bin"
    upload.write_bytes(b"fixture")
    gcid = hashlib.sha1(hashlib.sha1(upload.read_bytes()).digest()).hexdigest().upper()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    metadata = dict(api=1, upstream=head, version=version, capabilities=["folder_index", "drive_search_cached"],
                    source="github-actions" if "GITHUB_SHA" in os.environ else "local-worktree")

    # Each pair specifies a request and the v1 response fields it must preserve.
    cases = [
        ("{", {"code": "request"}),
        (dict(op="core_info"), {"value": metadata}),
        (dict(op="files"), {"code": "auth"}),
        (dict(op="drive_search_cached", query="x"), {"code": "auth"}),
        (dict(op="init", path=str(work / "missing.json")), {"value": False}),
        (dict(op="init", path=str(session)), {"value": True}),
        (dict(op="hash", path=str(upload)), {"value": gcid}),
        (dict(op="folder_stats_begin", scan="root"), {"value": None}),
        (dict(op="folder_stats_cancel", scan="root"), {"value": None}),
        (dict(op="folder_stats", scan="root", id=""), {"code": "cancelled"}),
    ]
    for kind in ("upload", "download"):
        transfer = dict(transfer=kind)
        progress = dict(bytes=0, total=0)
        if kind == "upload":
            progress["stage"] = ""
        cases.extend([
            (dict(op="transfer_begin", kind=kind, **transfer), {"value": None}),
            (dict(op="transfer_progress", **transfer), {"value": progress}),
            (dict(op="transfer_stop", **transfer), {"value": None}),
            (dict(op=kind, id="file", path=str(upload), **transfer), {"code": "request"}),
            (dict(op="transfer_end", **transfer), {"value": None}),
            (dict(op="transfer_progress", **transfer), {"code": "request"}),
        ])
    cases.extend([
        (dict(op="logout", path=str(session)), {"value": None}),
        (dict(op="init", path=str(session)), {"value": False}),
    ])

    lines = [req if isinstance(req, str) else json.dumps(req, ensure_ascii=False) for req, _ in cases]
    run = subprocess.run(java, cwd=work, input="\n".join(lines) + "\n",
                         text=True, encoding="utf-8", stdout=subprocess.PIPE, check=True, timeout=60)
    for (req, expected), line in zip(cases, run.stdout.splitlines(), strict=True):
        reply = json.loads(line)
        if "code" in expected:
            assert isinstance(reply.get("error"), str) and "value" not in reply, (req, reply)
        else:
            assert "error" not in reply, (req, reply)
        for key, value in expected.items():
            assert reply[key] == value, (req, reply, expected)
    assert not session.exists()
    print(f"{len(cases)} JNI requests passed. core_info = {json.dumps(metadata)}")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as tmp:
        smoke(Path(sys.argv[1]).resolve(strict=True), Path(tmp))
