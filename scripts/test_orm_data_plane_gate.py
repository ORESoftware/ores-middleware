from __future__ import annotations

import copy
import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.orm_catalog_gate import DIESEL_VERSION, SEA_ORM_VERSION, Discrepancy
from scripts.orm_data_plane_gate import (
    DATA_PLANE_LANES,
    EXPECTED_REJECTIONS,
    compare_data_plane_witnesses,
    render_data_plane_cargo,
    render_rust_data_plane,
    resolve_cli_path,
    validate_supported_model,
)
from scripts.schema_convergence import ROOT, parse_json_schema, parse_typespec


class OrmDataPlaneGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")
        self.typespec_model = parse_typespec(self.root)
        self.json_model = parse_json_schema(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def witness(self, lane: str) -> dict[str, object]:
        authority, orm, _ = DATA_PLANE_LANES[lane]
        return {
            "schema": "ores.orm-data-plane-witness/v1",
            "lane": lane,
            "authority": authority,
            "orm": orm,
            "witness": {
                "rows": [
                    {
                        "id": "row-full",
                        "tenantId": "tenant-a",
                        "idempotencyKey": "key-full",
                        "requestHash": "hash-row-full",
                        "status": "succeeded",
                        "responseStatus": 201,
                        "responseBody": "ok",
                        "createdAt": "2026-09-03T20:00:00Z",
                        "expiresAt": "2026-09-04T20:00:00Z",
                    },
                    {
                        "id": "row-null",
                        "tenantId": "tenant-a",
                        "idempotencyKey": "key-null",
                        "requestHash": "hash-row-null",
                        "status": "pending",
                        "responseStatus": None,
                        "responseBody": None,
                        "createdAt": "2026-09-03T20:00:00Z",
                        "expiresAt": "2026-09-04T20:00:00Z",
                    },
                ],
                "rejections": {name: True for name in EXPECTED_REJECTIONS},
            },
        }

    def test_current_models_are_supported_independently(self) -> None:
        validate_supported_model(self.typespec_model, "typespec")
        validate_supported_model(self.json_model, "json-schema-openapi")

    def test_generated_crate_pins_real_database_enabled_orms(self) -> None:
        cargo = render_data_plane_cargo()
        self.assertIn(f'version = "={DIESEL_VERSION}"', cargo)
        self.assertIn(f'version = "={SEA_ORM_VERSION}"', cargo)
        self.assertIn('"postgres"', cargo)
        self.assertIn('"sqlx-postgres"', cargo)
        self.assertIn('"runtime-tokio-rustls"', cargo)

    def test_generated_source_contains_four_real_row_paths(self) -> None:
        source = render_rust_data_plane(self.typespec_model, self.json_model)
        for module in (
            "typespec_diesel",
            "typespec_seaorm",
            "json_schema_openapi_diesel",
            "json_schema_openapi_seaorm",
        ):
            self.assertIn(f"mod {module}", source)
        for lane in DATA_PLANE_LANES:
            self.assertIn(f'"{lane}"', source)
        # Each of the two Diesel authority modules performs two positive
        # inserts plus primary-key, unique-key, and enum rejection inserts.
        self.assertEqual(source.count("diesel::insert_into"), 10)
        # SeaORM mirrors those same five insert attempts per authority.
        self.assertEqual(source.count(".insert(&database)"), 10)
        # Three raw database rejection cases per SeaORM authority.
        self.assertEqual(source.count("execute_unprepared"), 6)
        self.assertEqual(source.count("DeriveEntityModel"), 2)
        self.assertEqual(source.count("diesel::table!"), 2)

    def test_equal_four_way_witnesses_have_no_findings(self) -> None:
        witnesses = {lane: self.witness(lane) for lane in DATA_PLANE_LANES}
        findings: list[Discrepancy] = []
        compare_data_plane_witnesses(witnesses, findings)
        self.assertEqual(findings, [])

    def test_row_drift_stops_evaluation(self) -> None:
        witnesses = {lane: self.witness(lane) for lane in DATA_PLANE_LANES}
        witnesses = copy.deepcopy(witnesses)
        witnesses["json-schema-openapi-diesel"]["witness"]["rows"][0][
            "responseStatus"
        ] = 202
        findings: list[Discrepancy] = []
        compare_data_plane_witnesses(witnesses, findings)
        self.assertIn(
            "orm-data-plane-cross-lane-mismatch",
            {finding.kind for finding in findings},
        )

    def test_accepted_negative_case_stops_evaluation(self) -> None:
        witnesses = {lane: self.witness(lane) for lane in DATA_PLANE_LANES}
        witnesses["typespec-seaorm"]["witness"]["rejections"][
            "uniqueKey"
        ] = False
        findings: list[Discrepancy] = []
        compare_data_plane_witnesses(witnesses, findings)
        self.assertIn(
            "orm-data-plane-negative-case-accepted",
            {finding.kind for finding in findings},
        )

    def test_relative_output_paths_are_root_anchored(self) -> None:
        resolved = resolve_cli_path(
            self.root,
            Path("target/orm-data-plane"),
            self.root / "unused",
        )
        self.assertEqual(
            resolved,
            (self.root / "target/orm-data-plane").resolve(),
        )


if __name__ == "__main__":
    unittest.main()
