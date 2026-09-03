from __future__ import annotations

import copy
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.orm_matrix_gate import (
    LANES,
    compare_orm_matrix,
    expected_manifests,
    render_rust_matrix,
)
from scripts.orm_catalog_gate import Discrepancy
from scripts.schema_convergence import ROOT, parse_json_schema, parse_typespec


class OrmMatrixGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")
        self.typespec_model = parse_typespec(self.root)
        self.json_model = parse_json_schema(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_generated_source_contains_all_four_compiled_lanes(self) -> None:
        source = render_rust_matrix(self.typespec_model, self.json_model)
        for module in (
            "typespec_diesel",
            "typespec_seaorm",
            "json_schema_openapi_diesel",
            "json_schema_openapi_seaorm",
        ):
            self.assertIn(f"mod {module}", source)
        for lane in LANES:
            self.assertIn(f'"{lane}"', source)
        self.assertEqual(source.count("diesel::table!"), 2)
        self.assertEqual(source.count("DeriveEntityModel"), 2)

    def test_expected_manifests_preserve_authority_and_orm_cross_product(self) -> None:
        manifests = expected_manifests(self.typespec_model, self.json_model)
        self.assertEqual(set(manifests), set(LANES))
        self.assertEqual(manifests["typespec-diesel"]["sourceAuthority"], "typespec")
        self.assertEqual(manifests["typespec-seaorm"]["sourceAuthority"], "typespec")
        self.assertEqual(
            manifests["json-schema-openapi-diesel"]["sourceAuthority"],
            "json-schema-openapi",
        )
        self.assertEqual(
            manifests["json-schema-openapi-seaorm"]["sourceAuthority"],
            "json-schema-openapi",
        )
        self.assertEqual(manifests["typespec-diesel"]["orm"], "diesel")
        self.assertEqual(manifests["typespec-seaorm"]["orm"], "seaorm")

    def test_equal_matrix_has_no_findings(self) -> None:
        manifests = expected_manifests(self.typespec_model, self.json_model)
        findings: list[Discrepancy] = []
        compare_orm_matrix(manifests, copy.deepcopy(manifests), findings)
        self.assertEqual(findings, [])

    def test_cross_authority_diesel_type_drift_stops_evaluation(self) -> None:
        manifests = expected_manifests(self.typespec_model, self.json_model)
        actual = copy.deepcopy(manifests)
        actual["json-schema-openapi-diesel"]["rustTypes"][0]["type"] = "Vec<u8>"
        findings: list[Discrepancy] = []
        compare_orm_matrix(actual, manifests, findings)
        kinds = {finding.kind for finding in findings}
        self.assertIn("json-schema-openapi-diesel-compiled-manifest-mismatch", kinds)
        self.assertIn("diesel-cross-authority-type-mismatch", kinds)

    def test_same_authority_contract_drift_stops_evaluation(self) -> None:
        manifests = expected_manifests(self.typespec_model, self.json_model)
        actual = copy.deepcopy(manifests)
        actual["typespec-seaorm"]["contract"]["table"] = "wrong_table"
        findings: list[Discrepancy] = []
        compare_orm_matrix(actual, manifests, findings)
        kinds = {finding.kind for finding in findings}
        self.assertIn("typespec-seaorm-compiled-manifest-mismatch", kinds)
        self.assertIn("four-way-orm-contract-mismatch", kinds)


if __name__ == "__main__":
    unittest.main()
