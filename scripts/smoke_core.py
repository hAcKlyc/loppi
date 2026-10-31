#!/usr/bin/env python3
"""Exercise the installed-core protocol with isolated user data and no capture."""

import json
import os
import socket
import subprocess
import tempfile
import time
import uuid
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "target/debug/loppi"
CORE = ROOT / "target/debug/loppi-core"


def main() -> None:
    if not CLI.is_file() or not CORE.is_file():
        raise SystemExit("build loppi-core and loppi-cli before running this smoke test")
    with tempfile.TemporaryDirectory(prefix="loppi-smoke-") as directory:
        environment = os.environ | {"LOPPI_TEST_PARENT_PID": str(os.getpid()),
            "LOPPI_DATA_DIR": directory,
            "LOPPI_CORE_BIN": str(CORE),
        }

        def command(*args: str) -> str:
            return subprocess.check_output((str(CLI), *args), env=environment, text=True)

        try:
            initial = json.loads(command("status"))["state"]
            assert initial["enabled_intent"] is False
            assert initial["capture_state"] == "not_ready"
            assert initial["capture_blocker"] == "model_not_ready"
            assert initial["microphone_phase"] == "idle"
            assert initial["system_phase"] == "idle"
            source_id = str(uuid.uuid4())
            command("source", "microphone", "off", "--request-id", source_id, "--if-revision", "0")
            paused = json.loads(command("pause"))["state"]
            assert paused["paused_intent"] is True
            assert paused["microphone_enabled"] is False
            assert paused["revision"] == initial["revision"] + 2
            entries = [json.loads(line) for line in command("logs", "--component", "ipc").splitlines()]
            assert [entry["event"] for entry in entries] == [
                "request_received", "set_source", "request_received", "pause"
            ]
            # 一条请求的接收与处理记录统一归属发起方；关联靠 request_id。
            assert [entry["client"] for entry in entries] == ["cli", "cli", "cli", "cli"]
            assert all(entry["correlation_id"] == entry["request_id"] for entry in entries)
            archive_fifo = Path(directory, "logs/loppi.01.jsonl")
            os.mkfifo(archive_fifo)
            try:
                unreadable_logs = subprocess.run(
                    (str(CLI), "logs"), env=environment, text=True,
                    capture_output=True, timeout=5,
                )
                assert unreadable_logs.returncode == 2
                assert json.loads(command("status"))["state"]["revision"] == 2
            finally:
                archive_fifo.unlink()
            replay = json.loads(command("source", "microphone", "off", "--request-id", source_id, "--if-revision", "0"))["state"]
            # Status replies project current runtime; replay must not apply a second mutation.
            assert replay["revision"] == paused["revision"]
            assert replay["microphone_enabled"] is False
            assert json.loads(command("status"))["state"]["revision"] == 2
            resumed = json.loads(command("resume"))["state"]
            assert resumed["revision"] == 3 and resumed["paused_intent"] is False
            stale = subprocess.run(
                (str(CLI), "source", "system", "off", "--if-revision", "1"),
                env=environment, text=True, capture_output=True,
            )
            assert stale.returncode == 2 and "revision_conflict" in stale.stderr
            duplicate_id = subprocess.run(
                (str(CLI), "source", "microphone", "on", "--if-revision", "0", "--request-id", source_id),
                env=environment, text=True, capture_output=True,
            )
            assert duplicate_id.returncode == 2 and "idempotency_conflict" in duplicate_id.stderr
            assert json.loads(command("status"))["state"]["revision"] == 3
            with socket.socket(socket.AF_UNIX) as stalled:
                stalled.connect(str(Path(directory, "core.sock")))
                stalled.sendall(b"{")
                started = time.monotonic()
                assert json.loads(command("status"))["state"]["revision"] == 3
                assert time.monotonic() - started < 2
                for _ in range(4):
                    time.sleep(0.8)
                    try:
                        stalled.sendall(b" ")
                    except BrokenPipeError:
                        break
                stalled.settimeout(1)
                try:
                    assert stalled.recv(1) == b""
                except ConnectionResetError:
                    pass
            print("core/CLI smoke: isolated query, mutations, revision and correlated logs passed")
        finally:
            command("quit")

    with tempfile.TemporaryDirectory(prefix="loppi-no-logs-") as directory:
        Path(directory, "logs").write_text("blocking directory creation")
        environment = os.environ | {"LOPPI_TEST_PARENT_PID": str(os.getpid()),
            "LOPPI_DATA_DIR": directory,
            "LOPPI_CORE_BIN": str(CORE),
        }
        try:
            result = subprocess.check_output(
                (str(CLI), "status"), env=environment, text=True
            )
            assert json.loads(result)["state"]["capture_state"] == "not_ready"
            logs_result = subprocess.run(
                (str(CLI), "logs"), env=environment, text=True, capture_output=True
            )
            assert logs_result.returncode == 2
            assert "logs_unavailable" in logs_result.stderr
            print("core/CLI smoke: diagnosis storage failure does not block state queries")
        finally:
            subprocess.check_output((str(CLI), "quit"), env=environment)

    with tempfile.TemporaryDirectory(prefix="loppi-fifo-logs-") as directory:
        logs = Path(directory, "logs")
        logs.mkdir()
        os.mkfifo(logs / "loppi.jsonl")
        environment = os.environ | {"LOPPI_TEST_PARENT_PID": str(os.getpid()),
            "LOPPI_DATA_DIR": directory,
            "LOPPI_CORE_BIN": str(CORE),
        }
        try:
            result = subprocess.run(
                (str(CLI), "status"), env=environment, text=True,
                capture_output=True, timeout=5,
            )
            assert result.returncode == 0, result.stderr
            assert json.loads(result.stdout)["state"]["capture_state"] == "not_ready"
            print("core/CLI smoke: FIFO log path cannot block startup")
        finally:
            subprocess.check_output((str(CLI), "quit"), env=environment)

    with tempfile.TemporaryDirectory(prefix="loppi-login-handoff-") as directory:
        environment = os.environ | {"LOPPI_TEST_PARENT_PID": str(os.getpid()),
            "LOPPI_DATA_DIR": directory,
            "LOPPI_CORE_BIN": str(CORE),
        }
        on_demand = subprocess.Popen(
            (str(CORE), "serve", "--on-demand"), env=environment,
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
        )
        login = None
        try:
            socket_path = Path(directory, "core.sock")
            for _ in range(100):
                if socket_path.exists():
                    break
                time.sleep(0.02)
            assert socket_path.exists()
            login = subprocess.Popen(
                (str(CORE), "serve", "--login"), env=environment,
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True,
            )
            on_demand.wait(timeout=10)
            assert on_demand.returncode == 0, on_demand.stderr.read()
            result = None
            for _ in range(100):
                if socket_path.exists() and login.poll() is None:
                    try:
                        result = subprocess.run(
                            (str(CLI), "status"), env=environment, text=True,
                            capture_output=True, timeout=3,
                        )
                        if result.returncode == 0:
                            break
                    except subprocess.TimeoutExpired:
                        pass
                time.sleep(0.02)
            assert result is not None and result.returncode == 0, result.stderr if result else "login core did not respond"
            assert login.poll() is None
            print("core/CLI smoke: login core takes ownership from on-demand core")
        finally:
            if login is not None and login.poll() is None:
                subprocess.run((str(CLI), "quit"), env=environment, capture_output=True, timeout=5)
                login.wait(timeout=5)
            if on_demand.poll() is None:
                on_demand.terminate()
                on_demand.wait(timeout=5)


if __name__ == "__main__":
    main()
