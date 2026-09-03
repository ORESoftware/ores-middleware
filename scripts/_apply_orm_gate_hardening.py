#!/usr/bin/env python3
"""Apply the reviewed DEN-3959 ORM-entrypoint hardening.

This file is temporary build machinery. The one-time proof workflow removes it
in the same commit that lands the proven source changes.
"""

from __future__ import annotations

from pathlib import Path


def patch_compatibility_gate() -> None:
    gate = Path("scripts/orm_catalog_gate.py")
    text = gate.read_text(encoding="utf-8")
    future = "\nfrom __future__ import annotations\n"
    start = text.index('"""')
    end = text.index(future, start)
    doc = '''"""Compatibility helpers for the full peer-authority ORM matrix.

TypeSpec and JSON Schema/OpenAPI remain independent top-level authorities. Each
source must generate SQL plus both Diesel and SeaORM code. This module retains
shared catalog and code-generation helpers for compatibility; its executable
entrypoint delegates to ``scripts.orm_matrix_gate`` so no command can admit the
old asymmetric TypeSpec-to-Diesel / JSON-Schema-to-SeaORM pairing.
"""'''
    text = text[:start] + doc + text[end:]

    marker = "\ndef main() -> int:\n"
    count = text.count(marker)
    if count != 1:
        raise RuntimeError(f"expected one legacy main, found {count}")
    text = text.replace(marker, "\ndef legacy_main() -> int:\n", 1)

    entry = '''

def main() -> int:
    """Run the only supported four-way ORM and PostgreSQL admission gate."""
    from scripts.orm_matrix_gate import main as matrix_main

    return matrix_main()
'''
    footer = '\n\nif __name__ == "__main__":\n'
    if footer not in text:
        raise RuntimeError("missing orm_catalog_gate executable footer")
    text = text.replace(footer, entry + footer, 1)
    gate.write_text(text, encoding="utf-8")


def patch_tests() -> None:
    tests = Path("scripts/test_orm_catalog_gate.py")
    text = tests.read_text(encoding="utf-8")
    if "from unittest.mock import patch\n" not in text:
        text = text.replace(
            "import unittest\n",
            "import unittest\nfrom unittest.mock import patch\n",
            1,
        )

    method = '''
    def test_compatibility_entrypoint_delegates_to_full_matrix(self) -> None:
        from scripts.orm_catalog_gate import main

        with patch("scripts.orm_matrix_gate.main", return_value=17) as matrix_main:
            self.assertEqual(main(), 17)
        matrix_main.assert_called_once_with()
'''
    footer = '\n\nif __name__ == "__main__":\n'
    if method.strip() not in text:
        if footer not in text:
            raise RuntimeError("missing orm catalog test footer")
        text = text.replace(footer, method + footer, 1)
    tests.write_text(text, encoding="utf-8")


def patch_zpkg_guard() -> None:
    zpkg = Path("scripts/check_zpkg.py")
    text = zpkg.read_text(encoding="utf-8")
    anchor = '''    for required_path in required_paths:
        if not (root / required_path).is_file():
            errors.append(f"missing required convergence/package gate file: {required_path}")
'''
    addition = anchor + '''
    matrix_source = (root / "scripts/orm_matrix_gate.py").read_text(encoding="utf-8")
    for lane in (
        "typespec-diesel",
        "typespec-seaorm",
        "json-schema-openapi-diesel",
        "json-schema-openapi-seaorm",
    ):
        if lane not in matrix_source:
            errors.append(f"four-way ORM matrix must retain lane {lane!r}")

    compatibility_source = (root / "scripts/orm_catalog_gate.py").read_text(
        encoding="utf-8"
    )
    for required_text in (
        "from scripts.orm_matrix_gate import main as matrix_main",
        "return matrix_main()",
    ):
        if required_text not in compatibility_source:
            errors.append(
                "legacy ORM entrypoint must delegate to the complete "
                f"TypeSpec/JSON-Schema x Diesel/SeaORM matrix: missing {required_text!r}"
            )
'''
    count = text.count(anchor)
    if count != 1:
        raise RuntimeError(f"expected one zpkg required-path loop, found {count}")
    if "legacy ORM entrypoint must delegate to the complete" not in text:
        text = text.replace(anchor, addition, 1)
    zpkg.write_text(text, encoding="utf-8")


def patch_architecture() -> None:
    architecture = Path("docs/architecture.md")
    text = architecture.read_text(encoding="utf-8")
    section_start = text.index(
        "`scripts/schema_convergence.py` projects both lanes independently and compares:"
    )
    section_end = text.index(
        "\n## Header, path, and representation behavior",
        section_start,
    )
    replacement = '''`tools/contract-parity/src/bin/persistence_codegen.rs` reads each authored
source independently. Both lanes generate the same common products without
copying or translating one authority into the other:

1. normalized field, type, requiredness, enum, table, primary-key, and unique
   constraint semantics;
2. `SQL_T` and `SQL_J`;
3. TypeScript interfaces and executable validators;
4. Rust, Go, Gleam, Elixir, and Erlang types/runtime code;
5. `Diesel_T` and `SeaORM_T`; and
6. `Diesel_J` and `SeaORM_J`.

The only executable database admission path is `scripts/orm_matrix_gate.py`.
The older `scripts/orm_catalog_gate.py` remains a helper import for catalog and
code-generation functions, but its command-line entrypoint delegates to the
complete matrix. A green result can therefore never mean only
TypeSpec-to-Diesel and JSON-Schema-to-SeaORM were tested.

| Human-authored authority | Diesel | SeaORM | SQL/catalog |
| --- | --- | --- | --- |
| TypeSpec | `typespec-diesel` | `typespec-seaorm` | `SQL_T` / `typespec_lane` |
| JSON Schema/OpenAPI | `json-schema-openapi-diesel` | `json-schema-openapi-seaorm` | `SQL_J` / `json_schema_lane` |

The gate pins and compiles real Diesel and SeaORM implementations, executes all
four authority/ORM manifests, applies independently generated SQL to separate
disposable PostgreSQL schemas, and normalizes `pg_catalog` plus
`information_schema` read-back. It compares ORM semantics within each
authority, each ORM across authorities, and both database catalogs.

Any source, generated-code, ORM, SQL, compiler, runtime, or catalog mismatch
receives a deterministic fingerprint and enters `STOPPED_FOR_EVALUATION`.
Generation, publication, migration, dependency promotion, server adoption, and
deployment remain blocked. No authority or ORM wins by precedence or fallback.
'''
    text = text[:section_start] + replacement + text[section_end:]
    architecture.write_text(text, encoding="utf-8")


def main() -> None:
    patch_compatibility_gate()
    patch_tests()
    patch_zpkg_guard()
    patch_architecture()


if __name__ == "__main__":
    main()
