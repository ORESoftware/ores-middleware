import json
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from ores_mw_config import ManifestError, compile_manifest  # noqa: E402

STACK = {
    "contractVersion": "1.0.0",
    "environment": "test",
    "requiredCapabilities": [],
    "settings": {},
    "integrations": {},
}


def write_manifest(root: Path, *, middleware: str, name: str = "api") -> Path:
    stack_line = 'stack_config = "config/middleware.json"\n' if middleware == "stack" else ""
    path = root / ".ores-mw.toml"
    path.write_text(f'''\nschema_version = 1\nrepository_mode = "server-only"\n[[targets]]\nname = "{name}"\nrole = "server"\nroots = ["server"]\nmiddleware = "{middleware}"\n{stack_line}''', encoding="utf-8")
    return path


class OutputHardeningTests(unittest.TestCase):
    def test_compile_rejects_symlinked_output_component_without_touching_destination(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            (root / "server").mkdir()
            manifest = write_manifest(root, middleware="disabled")
            try:
                (root / "target").symlink_to(Path(outside), target_is_directory=True)
            except OSError:
                self.skipTest("symlink unavailable")
            with self.assertRaisesRegex(ManifestError, "output-symlink-not-allowed"):
                compile_manifest(manifest, root, Path("target/ores-mw"))
            self.assertFalse((Path(outside) / "ores-mw").exists())

    def test_recompile_prunes_stale_per_target_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "server").mkdir()
            (root / "config").mkdir()
            (root / "config/middleware.json").write_text(json.dumps(STACK), encoding="utf-8")
            manifest = write_manifest(root, middleware="stack")
            out = root / "target/ores-mw"
            compile_manifest(manifest, root, out)
            self.assertTrue((out / "targets/api.json").is_file())

            write_manifest(root, middleware="disabled", name="worker")
            compile_manifest(manifest, root, out)
            self.assertFalse((out / "targets/api.json").exists())
            self.assertFalse((out / "targets").exists())
            receipt = json.loads((out / "receipt.json").read_text(encoding="utf-8"))
            self.assertEqual([target["name"] for target in receipt["targets"]], ["worker"])

    def test_compile_refuses_to_delete_unrecognized_output_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "server").mkdir()
            manifest = write_manifest(root, middleware="disabled")
            out = root / "target/ores-mw"
            out.mkdir(parents=True)
            sentinel = out / "keep-me.txt"
            sentinel.write_text("not generated", encoding="utf-8")
            with self.assertRaisesRegex(ManifestError, "output-directory-not-dedicated"):
                compile_manifest(manifest, root, out)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "not generated")

    def test_compile_rejects_symlinked_generated_entry(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            (root / "server").mkdir()
            manifest = write_manifest(root, middleware="disabled")
            out = root / "target/ores-mw"
            out.mkdir(parents=True)
            victim = Path(outside) / "victim.json"
            victim.write_text('{"do":"not touch"}', encoding="utf-8")
            try:
                (out / "receipt.json").symlink_to(victim)
            except OSError:
                self.skipTest("symlink unavailable")
            with self.assertRaisesRegex(ManifestError, "output-entry-symlink-not-allowed"):
                compile_manifest(manifest, root, out)
            self.assertEqual(victim.read_text(encoding="utf-8"), '{"do":"not touch"}')


if __name__ == "__main__":
    unittest.main()
