from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

from scripts.subprocess_capture import run_command


class SubprocessCaptureTests(unittest.TestCase):
    def test_machine_stdout_is_not_contaminated_by_diagnostics(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = run_command(
                [
                    sys.executable,
                    "-c",
                    (
                        "import json,sys; "
                        "print('Compiling witness...', file=sys.stderr); "
                        "print(json.dumps({'lane':'diesel'}))"
                    ),
                ],
                Path(directory),
            )
        self.assertEqual(json.loads(output), {"lane": "diesel"})
        self.assertNotIn("Compiling", output)

    def test_failure_retains_both_output_channels(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(
                ValueError,
                "machine-output.*compiler-diagnostic",
            ):
                run_command(
                    [
                        sys.executable,
                        "-c",
                        (
                            "import sys; print('machine-output'); "
                            "print('compiler-diagnostic', file=sys.stderr); "
                            "raise SystemExit(7)"
                        ),
                    ],
                    Path(directory),
                )


if __name__ == "__main__":
    unittest.main()
