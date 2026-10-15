"""Exercise the real macOS footprint adapter, including its child enumeration."""
import ctypes
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


@unittest.skipUnless(sys.platform == "darwin", "macOS native adapter")
class NativeResourcesTest(unittest.TestCase):
    def test_background_footprint_includes_three_resident_children(self):
        with tempfile.TemporaryDirectory(prefix="hear-native-resources-") as directory:
            library = Path(directory) / "resources.dylib"
            subprocess.run([
                "clang", "-dynamiclib", "-fobjc-arc", "-mmacosx-version-min=13.0",
                "-framework", "Foundation", "-framework", "IOKit",
                str(ROOT / "crates/hear-core/native/resources.m"), "-o", str(library),
            ], check=True, capture_output=True, timeout=60)
            native = ctypes.CDLL(str(library))
            native.hear_process_footprint.argtypes = [ctypes.c_int]
            native.hear_process_footprint.restype = ctypes.c_uint64
            native.hear_background_footprint.restype = ctypes.c_uint64
            children = []
            try:
                for _ in range(3):
                    child = subprocess.Popen([
                        sys.executable, "-c",
                        "import sys; data=bytearray(b'x'*16777216); "
                        "print('ready',flush=True); sys.stdin.read(1); print(len(data))",
                    ], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
                    children.append(child)
                    self.assertEqual(child.stdout.readline().strip(), "ready")
                import os
                before = native.hear_process_footprint(os.getpid())
                sizes = [native.hear_process_footprint(child.pid) for child in children]
                self.assertTrue(all(size >= 16 * 1024 * 1024 for size in sizes))
                total = native.hear_background_footprint()
                after = native.hear_process_footprint(os.getpid())
                # Account for minor parent sampling movement, while requiring
                # every live child. The pre-fix count/sizeof(int) loop misses all 3.
                self.assertGreaterEqual(total, min(before, after) + sum(sizes) - 1024 * 1024)
            finally:
                for child in children:
                    child.kill()
                    child.communicate(timeout=10)


if __name__ == "__main__":
    unittest.main()
