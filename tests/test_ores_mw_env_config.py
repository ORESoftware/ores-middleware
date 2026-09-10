import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

from ores_mw_config import (  # noqa: E402
    ManifestError,
    check_referenced_files,
    normalize_manifest,
)


def manifest(text: str):
    import tomllib
    return normalize_manifest(tomllib.loads(text))


BASE = """
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[flags2env]
contract = ".cli-flags.toml"
require_audit = true
precedence = "argv-over-env"

[[env]]
name = "redis_url"
key = "REDIS_URL"
kind = "url"
required = true
secret = true
description = "Redis URL supplied only through the approved secret environment."

[[env]]
name = "port"
key = "PORT"
kind = "integer"
required = false
secret = false
default = "8080"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "disabled"
"""


class MiddlewareEnvContractTests(unittest.TestCase):
    def test_env_inventory_normalizes_without_secret_values(self):
        value = manifest(BASE)
        self.assertEqual(value["flags2env"]["contract"], ".cli-flags.toml")
        self.assertTrue(value["flags2env"]["requireAudit"])
        self.assertEqual(value["env"][0]["key"], "REDIS_URL")
        self.assertTrue(value["env"][0]["secret"])
        self.assertNotIn("defaultValue", value["env"][0])
        self.assertEqual(value["env"][1]["defaultValue"], "8080")

    def test_secret_plaintext_default_fails_closed(self):
        bad = BASE.replace(
            'description = "Redis URL supplied only through the approved secret environment."',
            'default = "redis://plaintext.invalid"\n'
            'description = "Redis URL supplied only through the approved secret environment."',
        )
        with self.assertRaisesRegex(ManifestError, "secret-default-forbidden"):
            manifest(bad)

    def test_env_binding_requires_canonical_flags2env_contract(self):
        bad = BASE.replace(
            'contract = ".cli-flags.toml"',
            'contract = "config/flags.toml"',
        )
        with self.assertRaisesRegex(ManifestError, "canonical-flags-contract-required"):
            manifest(bad)

    def test_secret_environment_key_must_not_be_a_cli_flag(self):
        value = manifest(BASE)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / ".cli-flags.toml").write_text(
                '[flags.redis-url]\n'
                'env = "REDIS_URL"\n'
                'type = "string"\n',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ManifestError, "secret-env-exposed-as-cli-flag"):
                check_referenced_files(value, root)

    def test_secret_environment_key_may_be_env_only_while_operational_flag_is_declared(self):
        value = manifest(BASE)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / ".cli-flags.toml").write_text(
                '[env]\n'
                'ignore = ["REDIS_URL"]\n\n'
                '[flags.port]\n'
                'env = "PORT"\n'
                'type = "integer"\n',
                encoding="utf-8",
            )
            check_referenced_files(value, root)


if __name__ == "__main__":
    unittest.main()
