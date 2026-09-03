from __future__ import annotations

import copy
import unittest

from scripts.check_live_rollout_evidence import load, validate


class LiveRolloutEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.document = load()

    def test_checked_in_receipt_passes(self) -> None:
        self.assertEqual(validate(self.document), [])

    def test_duplicate_repository_fails(self) -> None:
        document = copy.deepcopy(self.document)
        document["repositories"][1]["repository"] = document["repositories"][0]["repository"]
        self.assertTrue(any("duplicate repositories" in item for item in validate(document)))

    def test_short_revision_fails(self) -> None:
        document = copy.deepcopy(self.document)
        document["repositories"][0]["middlewareRevision"] = "84afb50b81ae"
        self.assertTrue(any("middlewareRevision" in item for item in validate(document)))

    def test_summary_drift_fails(self) -> None:
        document = copy.deepcopy(self.document)
        document["summary"]["repositories"] = 21
        self.assertTrue(any("summary does not match" in item for item in validate(document)))

    def test_temporary_control_default_fails(self) -> None:
        document = copy.deepcopy(self.document)
        document["defaults"]["temporaryRolloutControls"] = ["retry.yml"]
        self.assertTrue(any("defaults drifted" in item for item in validate(document)))


if __name__ == "__main__":
    unittest.main()
