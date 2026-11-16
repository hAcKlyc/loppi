"""Native install with a synthetic signed package and disposable installation path."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
from threading import Thread
import time


ROOT = Path.cwd()
WINDOWS = sys.platform == "win32"
VERSION = "0.1.30"
TARGET = "windows-x86_64" if WINDOWS else "darwin-aarch64"


def run(directory):
    marker = directory / "launched.txt"
    if WINDOWS:
        installer = directory / "installer.c"
        installer.write_text(
            '#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n'
            'int main(int argc, char **argv) {\n'
            '  for (int i = 1; i < argc; i++) {\n'
            '    if (!strcmp(argv[i], "/UPDATE")) {\n'
            '      const char *path = getenv("LOPPI_PROBE_MARKER");\n'
            '      FILE *output = path ? fopen(path, "w") : NULL;\n'
            '      if (!output) return 2;\n'
            '      fputs("synthetic installer launched", output);\n'
            '      return fclose(output);\n'
            '    }\n'
            '  }\n'
            '  return 1;\n'
            '}\n',
            encoding="ascii",
        )
        package = directory / "synthetic-setup.exe"
        subprocess.run(["cl", "/nologo", installer.name, f"/Fe:{package.name}"], cwd=directory, check=True)
        executable = directory / "current.exe"
        executable.touch()
    else:
        executable = directory / "Current.app" / "Contents" / "MacOS" / "loppi"
        executable.parent.mkdir(parents=True)
        executable.write_text("current app")
        executable.chmod(0o755)
        replacement = directory / "Replacement.app" / "Contents" / "MacOS" / "loppi"
        replacement.parent.mkdir(parents=True)
        replacement.write_text("updated app")
        replacement.chmod(0o755)
        package = directory / "synthetic.app.tar.gz"
        with tarfile.open(package, "w:gz") as archive:
            archive.add(replacement.parents[2], arcname="Replacement.app")

    signer = ROOT / "node_modules/@tauri-apps/cli/tauri.js"
    key = directory / "probe.key"
    subprocess.run(["node", str(signer), "signer", "generate", "--ci", "--password", "", "-w", str(key)],
                   stdout=subprocess.DEVNULL, check=True)
    signing = os.environ | {"TAURI_SIGNING_PRIVATE_KEY": key.read_text().strip(),
                            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD": ""}
    subprocess.run(["node", str(signer), "signer", "sign", "--app-version", VERSION, str(package)],
                   env=signing, stdout=subprocess.DEVNULL, check=True)
    key.unlink()
    signature = Path(f"{package}.sig").read_text().strip()

    class Feed(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/latest":
                contents = json.dumps({"version": VERSION, "platforms": {
                    TARGET: {"url": f"{address}/asset", "signature": signature}
                }}).encode()
                content_type = "application/json"
            elif self.path == "/asset":
                contents = package.read_bytes()
                content_type = "application/octet-stream"
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(contents)))
            self.end_headers()
            self.wfile.write(contents)

    with ThreadingHTTPServer(("127.0.0.1", 0), Feed) as server:
        address = f"http://127.0.0.1:{server.server_port}"
        worker = Thread(target=server.serve_forever, daemon=True)
        worker.start()
        env = os.environ | {
            "LOPPI_PROBE_MARKER": str(marker),
            "LOPPI_PROBE_STATUS": str(directory / "verify-status.txt"),
            "LOPPI_PROBE_EXECUTABLE": str(executable),
            "CARGO_INCREMENTAL": "0",
            "CARGO_PROFILE_DEV_DEBUG": "0",
            "TAURI_CONFIG": json.dumps({
                "identifier": "app.loppi.install-probe",
                "app": {"windows": []},
                "plugins": {"updater": {
                    "pubkey": Path(f"{key}.pub").read_text().strip(),
                    "endpoints": [f"{address}/latest"],
                    "dangerousInsecureTransportProtocol": True,
                    "requireSignedVersion": True,
                }},
            }),
        }
        try:
            subprocess.run(["cargo", "build", "--locked", "-p", "loppi-desktop",
                            "--bin", "loppi-desktop"], env=env, check=True)
            binary = ROOT / "target/debug" / ("loppi-desktop.exe" if WINDOWS else "loppi-desktop")
            result = subprocess.run([str(binary.resolve())], env=env, capture_output=True, text=True, timeout=60)
            print(result.stdout)
            print(result.stderr, file=sys.stderr)
            if WINDOWS and result.returncode == 0xC0000139:
                subprocess.run(["dumpbin", "/imports", str(binary)], check=False)
            result.check_returncode()
            assert (directory / "verify-status.txt").read_text() == "tampered signed update rejected"
            if WINDOWS:
                deadline = time.monotonic() + 15
                while not marker.exists() and time.monotonic() < deadline:
                    time.sleep(0.1)
                assert marker.read_text() == "synthetic installer launched"
                print("verified Windows update reached native installer with /UPDATE")
            else:
                assert executable.read_text() == "updated app"
                assert executable.parents[2].stat().st_mode & 0o777 == 0o755
                print("verified macOS update replaced the bundle at its existing path")
        finally:
            server.shutdown()
            worker.join()


with tempfile.TemporaryDirectory(prefix="loppi-install-probe-") as temporary:
    run(Path(temporary))
