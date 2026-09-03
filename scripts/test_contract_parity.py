from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.check_contract_parity import ROOT, run


class ContractParityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_current_peer_contracts_match(self) -> None:
        self.assertEqual(run(self.root), [])

    def test_enum_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts" / "docs-serving.schema.json"
        doc = json.loads(path.read_text(encoding="utf-8"))
        doc["$defs"]["DocsAction"]["enum"].remove("not-acceptable")
        path.write_text(json.dumps(doc), encoding="utf-8")
        discrepancies = run(self.root)
        self.assertTrue(discrepancies)
        self.assertTrue(any(item.kind == "peer-contract-mismatch" for item in discrepancies))

    def test_requiredness_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts" / "docs-serving.schema.json"
        doc = json.loads(path.read_text(encoding="utf-8"))
        doc["$defs"]["DocsRequest"]["required"].append("accept")
        path.write_text(json.dumps(doc), encoding="utf-8")
        discrepancies = run(self.root)
        self.assertTrue(any("model DocsRequest" in item.detail for item in discrepancies))

    def test_hierarchy_regression_stops_evaluation(self) -> None:
        path = self.root / "contracts" / "authority-topology.json"
        doc = json.loads(path.read_text(encoding="utf-8"))
        doc["prohibitedAuthorityEdges"] = []
        path.write_text(json.dumps(doc), encoding="utf-8")
        discrepancies = run(self.root)
        self.assertTrue(any(item.kind == "authority-topology" for item in discrepancies))


if __name__ == "__main__":
    unittest.main()
