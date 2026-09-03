from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.build_targets import assert_output_is_self_contained


class SelfContainedBuildOutputTests(unittest.TestCase):
    def test_regular_files_and_internal_links_are_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "target" / "elixir"
            root.mkdir(parents=True)
            payload = root / "deps" / "telemetry" / "src" / "telemetry.erl"
            payload.parent.mkdir(parents=True)
            payload.write_text("-module(telemetry).\n", encoding="utf-8")

            link = root / "_build" / "lib" / "telemetry" / "src"
            link.parent.mkdir(parents=True)
            link.symlink_to(payload.parent, target_is_directory=True)

            assert_output_is_self_contained(root)

    def test_link_outside_target_root_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "target" / "elixir"
            root.mkdir(parents=True)
            external = base / "source" / "deps" / "telemetry" / "src"
            external.mkdir(parents=True)

            link = root / "_build" / "lib" / "telemetry" / "src"
            link.parent.mkdir(parents=True)
            link.symlink_to(external, target_is_directory=True)

            with self.assertRaisesRegex(ValueError, "escapes target root"):
                assert_output_is_self_contained(root)

    def test_broken_link_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "target" / "elixir"
            root.mkdir(parents=True)
            link = root / "_build" / "lib" / "missing"
            link.parent.mkdir(parents=True)
            link.symlink_to(root / "deps" / "missing", target_is_directory=True)

            with self.assertRaisesRegex(ValueError, "broken symlink"):
                assert_output_is_self_contained(root)


if __name__ == "__main__":
    unittest.main()
