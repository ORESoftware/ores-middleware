from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scripts.build_targets import (
    assert_output_is_self_contained,
    materialize_node_runtime_dependencies,
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

    def test_node_runtime_closure_copies_only_lock_defined_production_packages(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            project = base / "project"
            output = base / "target" / "ts"
            project.mkdir()
            output.mkdir(parents=True)

            (project / "package.json").write_text(
                json.dumps(
                    {
                        "name": "fixture",
                        "dependencies": {"runtime-pkg": "1.2.3"},
                        "devDependencies": {"dev-pkg": "9.9.9"},
                    }
                ),
                encoding="utf-8",
            )
            (project / "package-lock.json").write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {
                            "": {"dependencies": {"runtime-pkg": "1.2.3"}},
                            "node_modules/runtime-pkg": {"version": "1.2.3"},
                            "node_modules/dev-pkg": {"version": "9.9.9", "dev": True},
                        },
                    }
                ),
                encoding="utf-8",
            )
            runtime = project / "node_modules" / "runtime-pkg"
            runtime.mkdir(parents=True)
            (runtime / "index.js").write_text("export const ready = true;\n", encoding="utf-8")
            development = project / "node_modules" / "dev-pkg"
            development.mkdir(parents=True)
            (development / "index.js").write_text("export const dev = true;\n", encoding="utf-8")

            materialize_node_runtime_dependencies(project, output)

            self.assertTrue((output / "node_modules/runtime-pkg/index.js").is_file())
            self.assertFalse((output / "node_modules/dev-pkg").exists())
            receipt = json.loads(
                (output / "runtime-dependencies.json").read_text(encoding="utf-8")
            )
            self.assertEqual(receipt["schema"], "ores.node-runtime-dependency-closure/v1")
            self.assertEqual(
                [item["path"] for item in receipt["packages"]],
                ["node_modules/runtime-pkg"],
            )
            self.assertEqual(len(receipt["packageLockSha256"]), 64)
            assert_output_is_self_contained(output)

    def test_missing_required_node_runtime_dependency_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            project = base / "project"
            output = base / "target" / "ts"
            project.mkdir()
            output.mkdir(parents=True)
            (project / "package.json").write_text(
                json.dumps({"name": "fixture", "dependencies": {"missing": "1.0.0"}}),
                encoding="utf-8",
            )
            (project / "package-lock.json").write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {
                            "": {"dependencies": {"missing": "1.0.0"}},
                            "node_modules/missing": {"version": "1.0.0"},
                        },
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ValueError, "required npm runtime dependency"):
                materialize_node_runtime_dependencies(project, output)


if __name__ == "__main__":
    unittest.main()
