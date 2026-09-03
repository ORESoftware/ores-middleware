from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.build_targets import (
    assert_output_is_self_contained,
    materialize_project_directory,
)


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

    def test_project_link_is_materialized_inside_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            source = base / "source" / "src"
            source.mkdir(parents=True)
            (source / "ores_middleware.erl").write_text(
                "-module(ores_middleware).\n",
                encoding="utf-8",
            )

            root = base / "target" / "erlang"
            destination = root / "_build" / "default" / "lib" / "ores_middleware" / "src"
            destination.parent.mkdir(parents=True)
            destination.symlink_to(source, target_is_directory=True)

            materialize_project_directory(destination, source, required=True)

            self.assertFalse(destination.is_symlink())
            self.assertEqual(
                (destination / "ores_middleware.erl").read_text(encoding="utf-8"),
                "-module(ores_middleware).\n",
            )
            assert_output_is_self_contained(root)

    def test_missing_optional_project_directory_becomes_empty(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "target" / "erlang"
            destination = root / "_build" / "default" / "lib" / "ores_middleware" / "include"
            missing_source = base / "source" / "include"
            destination.parent.mkdir(parents=True)
            destination.symlink_to(missing_source, target_is_directory=True)

            materialize_project_directory(destination, missing_source, required=False)

            self.assertTrue(destination.is_dir())
            self.assertFalse(destination.is_symlink())
            assert_output_is_self_contained(root)

    def test_missing_required_project_directory_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "target" / "erlang"
            destination = root / "_build" / "default" / "lib" / "ores_middleware" / "src"
            missing_source = base / "source" / "src"
            destination.parent.mkdir(parents=True)
            destination.symlink_to(missing_source, target_is_directory=True)

            with self.assertRaisesRegex(ValueError, "required project source directory"):
                materialize_project_directory(destination, missing_source, required=True)


if __name__ == "__main__":
    unittest.main()
