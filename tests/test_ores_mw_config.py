import json
import subprocess
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from ores_mw_config import (  # noqa: E402
    ManifestError,
    compile_manifest,
    load_manifest,
    normalize_manifest,
    resolve_target,
)


STACK = {
    "contractVersion": "1.0.0",
    "environment": "test",
    "requiredCapabilities": [],
    "settings": {},
    "integrations": {},
}


def manifest(text: str):
    import tomllib
    return normalize_manifest(tomllib.loads(text))


class ManifestTests(unittest.TestCase):
    def test_server_only_stack_normalizes(self):
        value = manifest('''
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "stack"
stack_config = "config/middleware.json"
''')
        self.assertEqual(value["repositoryMode"], "server-only")
        self.assertEqual(value["targets"][0]["stackConfig"], "config/middleware.json")
        self.assertTrue(value["targets"][0]["enabled"])
        self.assertFalse(value["allowOverlappingRoots"])

    def test_client_only_propagation(self):
        value = manifest('''
schema_version = 1
repository_mode = "client-only"
[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "propagation-only"
propagate_headers = ["traceparent", "x-request-id"]
''')
        self.assertEqual(value["targets"][0]["propagateHeaders"], ["traceparent", "x-request-id"])

    def test_client_cannot_apply_server_stack(self):
        with self.assertRaisesRegex(ManifestError, "client-stack-unsupported"):
            manifest('''
schema_version = 1
repository_mode = "client-only"
[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "stack"
stack_config = "config/middleware.json"
''')

    def test_hybrid_requires_both_roles(self):
        with self.assertRaisesRegex(ManifestError, "hybrid-requires-client-and-server"):
            manifest('''
schema_version = 1
repository_mode = "hybrid"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
''')

    def test_split_hybrid_resolves_by_path(self):
        value = manifest('''
schema_version = 1
repository_mode = "hybrid"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "disabled"
''')
        self.assertEqual(resolve_target(value, target_name=None, source_path="server/src/main.rs")["name"], "api")
        self.assertEqual(resolve_target(value, target_name=None, source_path="web/src/app.ts")["name"], "browser")

    def test_shared_root_requires_opt_in_and_explicit_target(self):
        base = '''
schema_version = 1
repository_mode = "hybrid"
{overlap}
[[targets]]
name = "api"
role = "server"
roots = ["."]
middleware = "disabled"
[[targets]]
name = "browser"
role = "client"
roots = ["."]
middleware = "disabled"
'''
        with self.assertRaisesRegex(ManifestError, "overlapping-target-roots"):
            manifest(base.format(overlap=""))
        value = manifest(base.format(overlap="allow_overlapping_roots = true"))
        with self.assertRaisesRegex(ManifestError, "ambiguous-target-explicit-name-required"):
            resolve_target(value, target_name=None, source_path="src/shared.ts")
        self.assertEqual(resolve_target(value, target_name="api", source_path=None)["role"], "server")

    def test_path_traversal_backslash_and_absolute_paths_are_rejected(self):
        for bad in ("../secret", "server/../secret", "/etc", "server\\secret"):
            with self.subTest(bad=bad):
                with self.assertRaises(ManifestError):
                    manifest(f'''
schema_version = 1
repository_mode = "server-only"
[[targets]]
name = "api"
role = "server"
roots = ["{bad.replace(chr(92), chr(92) * 2)}"]
middleware = "disabled"
''')

    def test_unknown_keys_fail_closed(self):
        with self.assertRaisesRegex(ManifestError, "unknown-top-level-key"):
            manifest('''
schema_version = 1
repository_mode = "server-only"
sneaky = true
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
''')

    def test_headers_must_be_lowercase_unique_tokens(self):
        for headers in ('["Traceparent"]', '["traceparent", "traceparent"]', '["bad header"]'):
            with self.subTest(headers=headers):
                with self.assertRaises(ManifestError):
                    manifest(f'''
schema_version = 1
repository_mode = "client-only"
[[targets]]
name = "browser"
role = "client"
roots = ["web"]
middleware = "propagation-only"
propagate_headers = {headers}
''')

    def test_disabled_target_cannot_be_default(self):
        with self.assertRaisesRegex(ManifestError, "default-target-must-be-enabled"):
            manifest('''
schema_version = 1
repository_mode = "server-only"
default_target = "old"
[[targets]]
name = "old"
role = "server"
roots = ["old"]
enabled = false
middleware = "disabled"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
''')

    def test_compile_binds_manifest_and_stack_config_digests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "server").mkdir()
            (root / "config").mkdir()
            (root / "config/middleware.json").write_text(json.dumps(STACK), encoding="utf-8")
            manifest_path = root / ".ores-mw.toml"
            manifest_path.write_text('''
schema_version = 1
repository_mode = "server-only"
default_target = "api"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "stack"
stack_config = "config/middleware.json"
''', encoding="utf-8")
            receipt = compile_manifest(manifest_path, root, root / "target")
            self.assertEqual(receipt["status"], "passed")
            self.assertRegex(receipt["manifestSourceSha256"], r"^[0-9a-f]{64}$")
            self.assertRegex(receipt["targets"][0]["stackConfigSourceSha256"], r"^[0-9a-f]{64}$")
            self.assertTrue((root / "target/manifest.json").is_file())
            self.assertTrue((root / "target/targets/api.json").is_file())
            self.assertEqual(json.loads((root / "target/receipt.json").read_text())["status"], "passed")

    def test_compile_rejects_symlinked_stack_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "server").mkdir()
            (root / "config").mkdir()
            actual = root / "config/actual.json"
            actual.write_text(json.dumps(STACK), encoding="utf-8")
            try:
                (root / "config/middleware.json").symlink_to(actual)
            except OSError:
                self.skipTest("symlink unavailable")
            manifest_path = root / ".ores-mw.toml"
            manifest_path.write_text('''
schema_version = 1
repository_mode = "server-only"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "stack"
stack_config = "config/middleware.json"
''', encoding="utf-8")
            with self.assertRaisesRegex(ManifestError, "symlink"):
                compile_manifest(manifest_path, root, root / "target")

    def test_manifest_loader_rejects_oversize_without_parsing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / ".ores-mw.toml"
            path.write_bytes(b"x" * (256 * 1024 + 1))
            with self.assertRaisesRegex(ManifestError, "file-too-large"):
                load_manifest(path)

    def test_compile_rejects_output_outside_repository(self):
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as outside:
            root = Path(directory)
            (root / "server").mkdir()
            manifest_path = root / ".ores-mw.toml"
            manifest_path.write_text("""
schema_version = 1
repository_mode = "server-only"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
""", encoding="utf-8")
            with self.assertRaisesRegex(ManifestError, "output-outside-repository"):
                compile_manifest(manifest_path, root, Path(outside) / "evidence")

    def test_failure_output_does_not_echo_manifest_values(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = "DO_NOT_ECHO_SENTINEL_12345"
            path = root / ".ores-mw.toml"
            path.write_text(f"""
schema_version = 1
repository_mode = "server-only"
secret_like_unknown = "{sentinel}"
[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
""", encoding="utf-8")
            result = subprocess.run(
                [
                    sys.executable,
                    str(Path(__file__).resolve().parents[1] / "scripts/ores_mw_config.py"),
                    "--manifest", str(path),
                    "--repo-root", str(root),
                    "check", "--no-file-check",
                ],
                text=True,
                capture_output=True,
                check=False,
                timeout=10,
            )
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(sentinel, result.stdout)
            self.assertNotIn(sentinel, result.stderr)
            self.assertEqual(json.loads(result.stdout)["code"], "unknown-top-level-key")

    def test_checked_in_example_toml_files_normalize(self):
        import tomllib
        root = Path(__file__).resolve().parents[1]
        paths = sorted((root / "fixtures/ores-mw").glob("*.toml"))
        self.assertGreaterEqual(len(paths), 3)
        for path in paths:
            with self.subTest(path=path.name):
                normalize_manifest(tomllib.loads(path.read_text(encoding="utf-8")))


if __name__ == "__main__":
    unittest.main()
