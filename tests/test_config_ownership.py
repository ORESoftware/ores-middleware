import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from check_config_ownership import OwnershipError, audit_repository  # noqa: E402


VALID_ZPKG = '''
[package]
org = "oresoftware"
name = "ownership-fixture"
version = "0.0.0"

[dependencies]
"oresoftware/ores-middleware" = "^0.1.0"
'''

VALID_MIDDLEWARE = '''
schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
'''


class ConfigOwnershipTests(unittest.TestCase):
    def make_repo(self, zpkg: str = VALID_ZPKG, middleware: str = VALID_MIDDLEWARE):
        temp = tempfile.TemporaryDirectory()
        root = Path(temp.name)
        (root / ".zpkg.toml").write_text(zpkg, encoding="utf-8")
        (root / ".ores-mw.toml").write_text(middleware, encoding="utf-8")
        return temp, root

    # 1. The checked-in repository must satisfy its own boundary.
    def test_checked_in_repository_passes(self):
        receipt = audit_repository(ROOT)
        self.assertEqual(receipt["status"], "passed")
        self.assertEqual(receipt["zpkgRuntimeMiddlewareTables"], 0)
        self.assertGreater(receipt["middlewareTargetCount"], 0)

    # 2-5. The exact misplaced sections that motivated this hardening fail closed.
    def test_zpkg_middleware_auth_is_rejected(self):
        temp, root = self.make_repo(VALID_ZPKG + '\n[middleware.auth]\nenabled = true\n')
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    def test_zpkg_middleware_rate_limit_is_rejected(self):
        temp, root = self.make_repo(VALID_ZPKG + '\n[middleware.rate_limit]\nwindow_seconds = 60\n')
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    def test_zpkg_middleware_cache_is_rejected(self):
        temp, root = self.make_repo(VALID_ZPKG + '\n[middleware.cache]\ndefault_ttl_seconds = 300\n')
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    def test_zpkg_middleware_payload_is_rejected(self):
        temp, root = self.make_repo(VALID_ZPKG + '\n[middleware.payload]\nmax_json_size_kb = 2048\n')
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    # 6-8. Nesting or aliasing the runtime-middleware table cannot bypass the guard.
    def test_nested_package_middleware_is_rejected(self):
        zpkg = VALID_ZPKG + '\n[package.middleware]\nenabled = true\n'
        temp, root = self.make_repo(zpkg)
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    def test_runtime_middleware_alias_is_rejected(self):
        zpkg = VALID_ZPKG + '\n[runtime_middleware]\nenabled = true\n'
        temp, root = self.make_repo(zpkg)
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    def test_hyphenated_middleware_runtime_alias_is_rejected(self):
        zpkg = VALID_ZPKG + '\n[middleware-runtime]\nenabled = true\n'
        temp, root = self.make_repo(zpkg)
        with temp, self.assertRaisesRegex(OwnershipError, "middleware-runtime-config-forbidden-in-zpkg"):
            audit_repository(root)

    # 9-10. The audit is semantic TOML parsing, not substring matching.
    def test_comments_and_strings_do_not_false_positive(self):
        zpkg = VALID_ZPKG + '\n# [middleware.auth]\n[package.repository]\nurl = "https://example.test/middleware/auth"\n'
        temp, root = self.make_repo(zpkg)
        with temp:
            self.assertEqual(audit_repository(root)["status"], "passed")

    def test_middleware_dependency_name_is_allowed(self):
        temp, root = self.make_repo()
        with temp:
            self.assertEqual(audit_repository(root)["zpkgRuntimeMiddlewareTables"], 0)

    # 11-13. Missing/malformed authority surfaces fail closed.
    def test_missing_zpkg_is_rejected(self):
        temp, root = self.make_repo()
        with temp:
            (root / ".zpkg.toml").unlink()
            with self.assertRaisesRegex(OwnershipError, "zpkg-unavailable"):
                audit_repository(root)

    def test_malformed_zpkg_is_rejected(self):
        temp, root = self.make_repo('[package\nname = "broken"\n')
        with temp, self.assertRaisesRegex(OwnershipError, "invalid-zpkg-toml"):
            audit_repository(root)

    def test_missing_ores_mw_is_rejected(self):
        temp, root = self.make_repo()
        with temp:
            (root / ".ores-mw.toml").unlink()
            with self.assertRaisesRegex(OwnershipError, "ores-mw-unavailable"):
                audit_repository(root)

    # 14. Detailed provider tables are not silently accepted as a new TOML authority.
    def test_unmodeled_provider_table_in_ores_mw_is_rejected(self):
        middleware = VALID_MIDDLEWARE + '\n[middleware.auth]\nenabled = true\n'
        temp, root = self.make_repo(middleware=middleware)
        with temp, self.assertRaisesRegex(OwnershipError, "ores-mw-unknown-top-level-key"):
            audit_repository(root)

    # 15. Secret values still cannot be smuggled into middleware env metadata.
    def test_secret_default_in_ores_mw_is_rejected(self):
        middleware = '''
schema_version = 1
repository_mode = "server-only"

[flags2env]
contract = ".cli-flags.toml"
require_audit = true
precedence = "argv-over-env"

[[env]]
name = "auth_secret"
key = "AUTH_SECRET"
kind = "string"
required = true
secret = true
default = "must-not-live-here"

[[targets]]
name = "api"
role = "server"
roots = ["server"]
middleware = "disabled"
'''
        temp, root = self.make_repo(middleware=middleware)
        with temp, self.assertRaisesRegex(OwnershipError, "ores-mw-secret-default-forbidden"):
            audit_repository(root)

    def test_cli_failure_is_bounded_and_does_not_echo_values(self):
        sentinel = "DO_NOT_ECHO_AUTH_TOKEN_123"
        zpkg = VALID_ZPKG + f'\n[middleware.auth]\ntoken = "{sentinel}"\n'
        temp, root = self.make_repo(zpkg=zpkg)
        with temp:
            result = subprocess.run(
                [sys.executable, str(ROOT / "scripts/check_config_ownership.py"), "--repo-root", str(root)],
                text=True,
                capture_output=True,
                check=False,
                timeout=10,
            )
            self.assertEqual(result.returncode, 2)
            self.assertNotIn(sentinel, result.stdout)
            self.assertNotIn(sentinel, result.stderr)
            payload = json.loads(result.stdout)
            self.assertEqual(payload["code"], "middleware-runtime-config-forbidden-in-zpkg")


if __name__ == "__main__":
    unittest.main()
