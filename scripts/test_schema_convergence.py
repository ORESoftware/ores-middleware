from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.schema_convergence import ROOT, run


class SchemaConvergenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def check(self):
        return run(self.root, self.root / "target" / "schema-convergence")[0]

    def test_current_authorities_converge(self) -> None:
        self.assertEqual(self.check(), [])

    def test_requiredness_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text())
        doc["$defs"]["IdempotencyRecord"]["required"].append("responseBody")
        path.write_text(json.dumps(doc))
        findings = self.check()
        self.assertTrue(any(item.kind == "peer-contract-type-mismatch" for item in findings))

    def test_sql_table_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text())
        doc["$defs"]["IdempotencyRecord"]["x-ores-sql"]["table"] = "wrong_table"
        path.write_text(json.dumps(doc))
        findings = self.check()
        self.assertTrue(any(item.kind == "generated-sql-mismatch" for item in findings))

    def test_enum_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text())
        doc["$defs"]["IdempotencyStatus"]["enum"].remove("failed")
        path.write_text(json.dumps(doc))
        findings = self.check()
        self.assertTrue(any(item.kind == "peer-contract-type-mismatch" for item in findings))


if __name__ == "__main__":
    unittest.main()
