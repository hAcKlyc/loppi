#!/usr/bin/env python3
"""Offline end-to-end Agent contract. Temporary data only; no capture or cloud."""
import concurrent.futures
import json
import os
from pathlib import Path
import selectors
import socket
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "target/debug/hear"
CORE = ROOT / "target/debug/hear-core"


def main():
    with tempfile.TemporaryDirectory(prefix="hear-agent-smoke-") as temporary:
        root = Path(temporary)
        data = root / "data"
        env = os.environ | {"HEAR_DATA_DIR": str(data), "HEAR_CORE_BIN": str(CORE)}

        def cli(*args, code=0):
            p = subprocess.run([str(CLI), *args], env=env, capture_output=True, text=True, timeout=10)
            assert p.returncode == code, (args, p.stdout, p.stderr)
            return json.loads(p.stdout)

        assert cli("context", "status")["data"]["online"] is False
        assert cli("context", "list", code=3)["error"]["code"] == "core_unavailable"
        assert not (data / "core.sock").exists()
        assert not (data / "core.lock").exists()
        assert cli("context", "read", code=2)["error"]["code"] == "invalid_argument"
        assert cli("context", "list", "--limit", "bad", code=2)["error"]["code"] == "invalid_argument"
        info = cli("integrations", "inspect")
        assert Path(info["cli_path"]) == CLI.resolve()
        assert cli("integrations", "configure", "--target", "codex")["applied"] is False
        skill = root / "skill folder"
        cli("integrations", "install-skill", "--destination", str(skill))
        cli("integrations", "install-skill", "--destination", str(skill))
        assert str(CLI) in (skill / "references/cli.md").read_text()
        (skill / "SKILL.md").write_text("custom")
        cli("integrations", "install-skill", "--destination", str(skill), code=2)
        assert (skill / "SKILL.md").read_text() == "custom"
        command_dir = root / "bin"
        cli("integrations", "install-cli", "--directory", str(command_dir))
        assert (command_dir / "hear").resolve() == CLI.resolve()
        (command_dir / "hear").unlink()
        (command_dir / "hear").write_text("keep")
        cli("integrations", "install-cli", "--directory", str(command_dir), code=2)
        assert (command_dir / "hear").read_text() == "keep"
        # Protocol-20 Core decodes the whole request before checking version:
        # Context closes with no reply; stable lifecycle inspection exposes its version.
        data.mkdir(exist_ok=True)
        listener = socket.socket(socket.AF_UNIX)
        listener.bind(str(data / "core.sock"))
        listener.listen()
        listener.settimeout(.1)
        stop = threading.Event()

        def old_core():
            while not stop.is_set():
                try:
                    connection, _ = listener.accept()
                except socket.timeout:
                    continue
                with connection:
                    raw = b""
                    while chunk := connection.recv(65536):
                        raw += chunk
                    request = json.loads(raw)
                    if request["request"]["type"] == "lifecycle":
                        assert request["request"]["action"] == "inspect"
                        response = {"type": "lifecycle", "id": request["id"], "build": {"component": "core", "version": "0.1.8", "protocol_version": 20, "git_sha": "fixture", "dirty": False, "built_at_epoch": 0}, "allow_capture": False, "normal_run": False}
                        connection.sendall(json.dumps(response).encode())
                    elif request["request"]["type"] == "status":
                        # Stable Status intentionally succeeds across protocol versions;
                        # the implementation must inspect lifecycle identity instead.
                        raise AssertionError("Status cannot determine protocol compatibility")

        old = threading.Thread(target=old_core)
        old.start()
        try:
            for command in [("status",), ("search", "fixture")]:
                assert cli("context", *command, code=3)["error"]["code"] == "core_incompatible"
            lines = [
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "fixture", "version": "1"}}},
                {"jsonrpc": "2.0", "method": "notifications/initialized"},
                {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "hear_status"}},
            ]
            result = subprocess.run([str(CLI), "mcp", "serve"], input="".join(json.dumps(v)+"\n" for v in lines), env=env, text=True, capture_output=True, timeout=10)
            assert result.returncode == 0
            assert json.loads(result.stdout.splitlines()[-1])["result"]["structuredContent"]["error"]["code"] == "core_incompatible"
        finally:
            stop.set()
            old.join(timeout=2)
            listener.close()
            (data / "core.sock").unlink()
        core = subprocess.Popen([str(CORE), "serve", "--on-demand"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        mcp = None
        try:
            deadline = time.monotonic() + 10
            while not (data / "core.sock").exists():
                assert time.monotonic() < deadline and core.poll() is None
                time.sleep(.05)
            assert cli("context", "status")["data"]["online"] is True
            before = cli("status")["state"]
            mcp = subprocess.Popen([str(CLI), "mcp", "serve"], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            watcher = selectors.DefaultSelector()
            watcher.register(mcp.stdout, selectors.EVENT_READ)
            number = 0

            def rpc(method, params=None, notification=False):
                nonlocal number
                number += 1
                message = {"jsonrpc": "2.0", "method": method}
                if not notification:
                    message["id"] = number
                if params is not None:
                    message["params"] = params
                mcp.stdin.write(json.dumps(message) + "\n")
                mcp.stdin.flush()
                if notification:
                    return
                assert watcher.select(8), "MCP response timed out"
                result = json.loads(mcp.stdout.readline())
                assert result["id"] == number
                return result

            init = rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "fixture", "version": "1"}})
            assert init["result"]["capabilities"] == {"tools": {}}
            rpc("notifications/initialized", notification=True)
            tools = rpc("tools/list")["result"]["tools"]
            assert len(tools) == 6 and all(t["annotations"]["readOnlyHint"] for t in tools)
            for name, args, command in [
                ("hear_list_context", {"time": {"date": "today"}}, ["list", "--date", "today"]),
                ("hear_search_context", {"query": "fixture"}, ["search", "fixture"]),
                ("hear_read_context", {"selector": {"time": {"date": "today"}}}, ["read", "--date", "today"]),
                ("hear_list_people", {}, ["people"]),
            ]:
                result = rpc("tools/call", {"name": name, "arguments": args})["result"]
                assert result["isError"] is False
                value = result["structuredContent"]
                assert json.loads(result["content"][0]["text"]) == value
                assert value["data"] == cli("context", *command)["data"]
            invalid = rpc("tools/call", {"name": "hear_read_context", "arguments": {}})["result"]
            assert invalid["isError"] is True
            assert invalid["structuredContent"]["error"]["code"] == "invalid_argument"
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                assert all(v["ok"] for v in pool.map(lambda _: cli("context", "list"), range(12)))
            after = cli("status")["state"]
            assert before["revision"] == after["revision"]
            assert after["enabled_intent"] is False
            assert after["microphone_phase"] == after["system_phase"] == "idle"
            mcp.stdin.close()
            mcp.wait(timeout=5)
            assert mcp.returncode == 0 and mcp.stderr.read() == ""
        finally:
            if mcp is not None and mcp.poll() is None:
                mcp.terminate()
                mcp.wait(timeout=5)
            core.terminate()
            core.wait(timeout=5)
        print("PASS: existing-only, CLI errors, six MCP tools, CLI/MCP parity, concurrent reads, unchanged capture, installation preservation")


if __name__ == "__main__":
    main()
