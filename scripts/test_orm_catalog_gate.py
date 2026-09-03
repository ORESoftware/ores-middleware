from __future__ import annotations

import shutil
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from scripts.orm_catalog_gate import (
    DIESEL_VERSION,
    SEA_ORM_VERSION,
    diesel_rust_type,
    expected_columns,
    normalize_index_definition,
    orm_manifest,
    redact_database_url,
    render_cargo,
    render_rust,
    seaorm_rust_type,
    validate_catalog,
)
from scripts.schema_convergence import ROOT, parse_json_schema, parse_typespec


class OrmCatalogGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        shutil.copytree(ROOT / "contracts", self.root / "contracts")
        self.typespec_model = parse_typespec(self.root)
        self.json_model = parse_json_schema(self.root)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_generated_crate_pins_real_orm_versions(self) -> None:
        cargo = render_cargo()
        self.assertIn(f'version = "={DIESEL_VERSION}"', cargo)
        self.assertIn(f'version = "={SEA_ORM_VERSION}"', cargo)
        self.assertIn('features = ["postgres", "chrono"]', cargo)
        self.assertIn('features = ["macros", "with-chrono"]', cargo)

    def test_generated_source_uses_real_diesel_and_seaorm_macros(self) -> None:
        source = render_rust(self.typespec_model, self.json_model)
        self.assertIn("diesel::table!", source)
        self.assertIn("diesel::Queryable", source)
        self.assertIn("diesel::Selectable", source)
        self.assertIn("diesel::Insertable", source)
        self.assertIn("diesel::Identifiable", source)
        self.assertIn("DeriveEntityModel", source)
        self.assertIn("DeriveRelation", source)
        self.assertIn("impl ActiveModelBehavior for ActiveModel", source)

    def test_manifests_retain_independent_source_authorities(self) -> None:
        diesel = orm_manifest(
            self.typespec_model,
            "diesel",
            "typespec",
            diesel_rust_type,
        )
        seaorm = orm_manifest(
            self.json_model,
            "seaorm",
            "json-schema-openapi",
            seaorm_rust_type,
        )
        self.assertEqual(diesel["sourceAuthority"], "typespec")
        self.assertEqual(seaorm["sourceAuthority"], "json-schema-openapi")
        self.assertEqual(diesel["contract"], seaorm["contract"])

    def test_expected_catalog_columns_preserve_types_and_nullability(self) -> None:
        columns = expected_columns(self.typespec_model)
        by_name = {item["column"]: item for item in columns}
        self.assertEqual(by_name["response_status"]["dataType"], "integer")
        self.assertEqual(by_name["response_status"]["udtName"], "int4")
        self.assertTrue(by_name["response_status"]["nullable"])
        self.assertEqual(by_name["created_at"]["dataType"], "timestamp with time zone")
        self.assertFalse(by_name["created_at"]["nullable"])

    def test_catalog_validation_detects_missing_constraints_and_indexes(self) -> None:
        catalog = {
            "table": self.typespec_model.table,
            "columns": expected_columns(self.typespec_model),
            "constraints": [],
            "indexes": [],
        }
        findings = []
        validate_catalog(self.typespec_model, catalog, "typespec", findings)
        kinds = {item.kind for item in findings}
        self.assertIn("typespec-catalog-constraint-mismatch", kinds)
        self.assertIn("typespec-catalog-index-mismatch", kinds)

    def test_index_normalization_removes_only_lane_schema(self) -> None:
        value = (
            "CREATE UNIQUE INDEX pk_middleware_idempotency "
            "ON typespec_lane.middleware_idempotency USING btree (id)"
        )
        normalized = normalize_index_definition(value, "typespec_lane")
        self.assertIn("ON middleware_idempotency", normalized)
        self.assertNotIn("typespec_lane.", normalized)

    def test_database_url_receipt_excludes_credentials_and_query(self) -> None:
        target = redact_database_url(
            "postgresql://sensitive-user:sensitive-password@db.example.test:5433/app?sslmode=require"
        )
        rendered = repr(target)
        self.assertEqual(target["host"], "db.example.test")
        self.assertEqual(target["port"], 5433)
        self.assertEqual(target["database"], "app")
        self.assertNotIn("sensitive-user", rendered)
        self.assertNotIn("sensitive-password", rendered)
        self.assertNotIn("sslmode", rendered)

    def test_compatibility_entrypoint_delegates_to_full_matrix(self) -> None:
        from scripts.orm_catalog_gate import main

        with patch("scripts.orm_matrix_gate.main", return_value=17) as matrix_main:
            self.assertEqual(main(), 17)
        matrix_main.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()
