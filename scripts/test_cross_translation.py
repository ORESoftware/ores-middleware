from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.cross_translate import (
    ROOT,
    canonical,
    parse_json_schema_document,
    parse_typespec_source,
    render_json_schema,
    render_typespec,
    run,
)


class CrossTranslationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_current_authorities_cross_translate_and_round_trip(self) -> None:
        findings, report = run(self.root, self.root / "target/cross-translation")
        self.assertEqual(findings, [])
        self.assertEqual(report["status"], "passed")
        self.assertFalse(report["generatedWitnessPolicy"]["authoritative"])
        self.assertFalse(report["generatedWitnessPolicy"]["mayRewriteHumanAuthoredSource"])
        self.assertEqual(len(report["artifacts"]), 4)

    def test_peer_requiredness_drift_stops_evaluation(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text(encoding="utf-8"))
        doc["$defs"]["IdempotencyRecord"]["required"].append("responseBody")
        path.write_text(json.dumps(doc), encoding="utf-8")
        findings, report = run(self.root, self.root / "target/cross-translation")
        self.assertTrue(
            any(item.kind == "peer-authority-cross-translation-mismatch" for item in findings)
        )
        self.assertEqual(report["status"], "stopped_for_evaluation")

    def test_typespec_to_json_shadow_tampering_is_detectable(self) -> None:
        source = (self.root / "contracts/persistence/idempotency-record.tsp").read_text()
        model = parse_typespec_source(source)
        shadow = render_json_schema(
            model, source_authority="typespec", source_digest="0" * 64
        )
        shadow["$defs"][model.name]["required"].remove("id")
        parsed = parse_json_schema_document(shadow)
        self.assertNotEqual(canonical(model), canonical(parsed))

    def test_json_schema_to_typespec_shadow_tampering_is_detectable(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text())
        model = parse_json_schema_document(doc)
        shadow = render_typespec(
            model, source_authority="json-schema-openapi", source_digest="0" * 64
        )
        shadow = shadow.replace("  responseBody?: string;", "  responseBody: string;")
        parsed = parse_typespec_source(shadow)
        self.assertNotEqual(canonical(model), canonical(parsed))

    def test_unsupported_constructs_fail_closed(self) -> None:
        path = self.root / "contracts/persistence/idempotency-record.schema.json"
        doc = json.loads(path.read_text())
        doc["$defs"]["IdempotencyRecord"]["properties"]["payload"] = {
            "type": "array",
            "items": {"type": "string"},
        }
        with self.assertRaisesRegex(ValueError, "unsupported JSON Schema property"):
            parse_json_schema_document(doc)


if __name__ == "__main__":
    unittest.main()
