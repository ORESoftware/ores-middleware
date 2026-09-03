use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const TYPESPEC_SOURCE: &str = "contracts/persistence/idempotency-record.tsp";
const JSON_SCHEMA_SOURCE: &str =
    "contracts/persistence/idempotency-record.schema.json";
const COMMON_ARTIFACTS: &[&str] = &[
    "model.json",
    "sql/schema.sql",
    "typescript/idempotency_record.d.ts",
    "typescript/idempotency_record.mjs",
    "rust/idempotency_record.rs",
    "golang/idempotency_record.go",
    "gleam/idempotency_record.gleam",
    "elixir/idempotency_record.ex",
    "erlang/idempotency_record.erl",
    "diesel/idempotency_record.rs",
    "seaorm/idempotency_record.rs",
];

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Field {
    name: String,
    column: String,
    logical_type: String,
    sql_type: String,
    nullable: bool,
    enum_values: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Model {
    name: String,
    table: String,
    primary_key: Vec<String>,
    unique: Vec<Vec<String>>,
    fields: Vec<Field>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Authority {
    TypeSpec,
    JsonSchemaOpenApi,
}

impl Authority {
    fn id(self) -> &'static str {
        match self {
            Self::TypeSpec => "typespec",
            Self::JsonSchemaOpenApi => "json-schema-openapi",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Self::TypeSpec => TYPESPEC_SOURCE,
            Self::JsonSchemaOpenApi => JSON_SCHEMA_SOURCE,
        }
    }

    fn output_folder(self) -> &'static str {
        self.id()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Discrepancy {
    fingerprint: String,
    kind: String,
    detail: String,
    owner: String,
    resolution_state: String,
}

impl Discrepancy {
    fn new(kind: impl Into<String>, detail: impl Into<String>) -> Self {
        let kind = kind.into();
        let detail = detail.into();
        let mut digest = Sha256::new();
        digest.update(kind.as_bytes());
        digest.update([0]);
        digest.update(detail.as_bytes());
        Self {
            fingerprint: format!("{:x}", digest.finalize()),
            kind,
            detail,
            owner: "ORESoftware/ores-middleware".to_owned(),
            resolution_state: "unexplained".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactReceipt {
    path: String,
    sha256: String,
    bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaneReceipt {
    authority: String,
    source: String,
    source_sha256: String,
    output_root: String,
    artifacts: Vec<ArtifactReceipt>,
}

#[derive(Clone, Debug)]
struct LaneOutput {
    authority: Authority,
    model: Model,
    artifacts: BTreeMap<String, String>,
}

fn sha256_bytes(value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(value);
    format!("{:x}", digest.finalize())
}

fn snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn pascal_case(value: &str) -> String {
    value
        .split(|character: char| character == '_' || character == '-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + characters.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}

fn extract_typespec_block(source: &str, keyword: &str, name: &str) -> Result<String> {
    let expression = format!(
        r"(?ms)\b{}\s+{}\s*\{{(?P<body>.*?)^\s*\}}",
        regex::escape(keyword),
        regex::escape(name)
    );
    let regex = Regex::new(&expression)?;
    regex
        .captures(source)
        .and_then(|captures| captures.name("body"))
        .map(|body| body.as_str().to_owned())
        .ok_or_else(|| format!("missing TypeSpec {keyword} {name}").into())
}

fn parse_typespec_enum(source: &str, name: &str) -> Result<Vec<String>> {
    let body = extract_typespec_block(source, "enum", name)?;
    let member = Regex::new(
        r#"^[A-Za-z_][A-Za-z0-9_]*\s*:\s*"([^"]+)"\s*,?$"#,
    )?;
    let mut values = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let captures = member
            .captures(line)
            .ok_or_else(|| format!("unsupported TypeSpec enum member: {line}"))?;
        values.push(captures[1].to_owned());
    }
    if values.is_empty() {
        return Err(format!("TypeSpec enum {name} must not be empty").into());
    }
    Ok(values)
}

fn typespec_metadata(source: &str, key: &str) -> Result<String> {
    let expression = format!(
        r"(?m)^\s*//\s*@ores\.sql\.{}\s+(.+?)\s*$",
        regex::escape(key)
    );
    let regex = Regex::new(&expression)?;
    regex
        .captures(source)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_owned())
        .ok_or_else(|| format!("missing TypeSpec SQL metadata {key}").into())
}

fn typespec_field(
    name: &str,
    raw_type: &str,
    nullable: bool,
    enums: &BTreeMap<String, Vec<String>>,
) -> Result<Field> {
    let raw_type = raw_type.trim();
    let (logical_type, sql_type, enum_values) = match raw_type {
        "string" => ("string", "text", Vec::new()),
        "int32" => ("int32", "integer", Vec::new()),
        "utcDateTime" => ("datetime", "timestamptz", Vec::new()),
        other if enums.contains_key(other) => (
            "enum",
            "text",
            enums
                .get(other)
                .cloned()
                .ok_or_else(|| format!("missing TypeSpec enum {other}"))?,
        ),
        other => return Err(format!("unsupported TypeSpec persistence type {other}").into()),
    };
    Ok(Field {
        name: name.to_owned(),
        column: snake_case(name),
        logical_type: logical_type.to_owned(),
        sql_type: sql_type.to_owned(),
        nullable,
        enum_values,
    })
}

fn parse_typespec(root: &Path) -> Result<Model> {
    let source = fs::read_to_string(root.join(TYPESPEC_SOURCE))?;
    let enum_values = parse_typespec_enum(&source, "IdempotencyStatus")?;
    let enums = BTreeMap::from([("IdempotencyStatus".to_owned(), enum_values)]);
    let property = Regex::new(
        r"^([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);$",
    )?;
    let mut fields = Vec::new();
    for raw in extract_typespec_block(
        &source,
        "model",
        "IdempotencyRecord",
    )?
    .lines()
    {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let captures = property
            .captures(line)
            .ok_or_else(|| format!("unsupported TypeSpec persistence property: {line}"))?;
        fields.push(typespec_field(
            &captures[1],
            &captures[3],
            captures.get(2).is_some(),
            &enums,
        )?);
    }
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    let primary_key = typespec_metadata(&source, "primary-key")?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let unique = typespec_metadata(&source, "unique")?
        .split(';')
        .filter(|group| !group.trim().is_empty())
        .map(|group| {
            group
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .collect();
    validate_model(&Model {
        name: "IdempotencyRecord".to_owned(),
        table: typespec_metadata(&source, "table")?,
        primary_key,
        unique,
        fields,
    })
}

fn json_field(
    name: &str,
    property: &Value,
    required: bool,
    definitions: &Map<String, Value>,
) -> Result<Field> {
    let object = property
        .as_object()
        .ok_or_else(|| format!("JSON Schema property {name} must be an object"))?;
    let (logical_type, sql_type, enum_values) = if let Some(reference) =
        object.get("$ref").and_then(Value::as_str)
    {
        let definition_name = reference.rsplit('/').next().unwrap_or(reference);
        let values = definitions
            .get(definition_name)
            .and_then(|value| value.get("enum"))
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("JSON Schema enum reference {definition_name} is invalid")
            })?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        format!("JSON Schema enum {definition_name} contains a non-string")
                    })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ("enum", "text", values)
    } else {
        match object.get("type").and_then(Value::as_str) {
            Some("string")
                if object.get("format").and_then(Value::as_str)
                    == Some("date-time") =>
            {
                ("datetime", "timestamptz", Vec::new())
            }
            Some("string") => ("string", "text", Vec::new()),
            Some("integer")
                if object.get("minimum").and_then(Value::as_i64)
                    == Some(i32::MIN as i64)
                    && object.get("maximum").and_then(Value::as_i64)
                        == Some(i32::MAX as i64) =>
            {
                ("int32", "integer", Vec::new())
            }
            _ => {
                return Err(format!(
                    "unsupported JSON Schema persistence property {name}: {property}"
                )
                .into());
            }
        }
    };
    Ok(Field {
        name: name.to_owned(),
        column: snake_case(name),
        logical_type: logical_type.to_owned(),
        sql_type: sql_type.to_owned(),
        nullable: !required,
        enum_values,
    })
}

fn parse_json_schema(root: &Path) -> Result<Model> {
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join(JSON_SCHEMA_SOURCE))?)?;
    let definitions = schema
        .get("$defs")
        .and_then(Value::as_object)
        .ok_or("JSON Schema must contain $defs")?;
    let model = definitions
        .get("IdempotencyRecord")
        .and_then(Value::as_object)
        .ok_or("JSON Schema must contain $defs.IdempotencyRecord")?;
    let required: BTreeSet<&str> = model
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let properties = model
        .get("properties")
        .and_then(Value::as_object)
        .ok_or("IdempotencyRecord must contain properties")?;
    let mut fields = properties
        .iter()
        .map(|(name, property)| {
            json_field(
                name,
                property,
                required.contains(name.as_str()),
                definitions,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    let sql = model
        .get("x-ores-sql")
        .and_then(Value::as_object)
        .ok_or("IdempotencyRecord must contain x-ores-sql")?;
    let primary_key = string_array(
        sql.get("primaryKey")
            .ok_or("x-ores-sql.primaryKey is required")?,
        "x-ores-sql.primaryKey",
    )?;
    let unique = sql
        .get("unique")
        .and_then(Value::as_array)
        .ok_or("x-ores-sql.unique is required")?
        .iter()
        .enumerate()
        .map(|(index, group)| {
            string_array(group, &format!("x-ores-sql.unique[{index}]"))
        })
        .collect::<Result<Vec<_>>>()?;
    validate_model(&Model {
        name: "IdempotencyRecord".to_owned(),
        table: sql
            .get("table")
            .and_then(Value::as_str)
            .ok_or("x-ores-sql.table is required")?
            .to_owned(),
        primary_key,
        unique,
        fields,
    })
}

fn string_array(value: &Value, label: &str) -> Result<Vec<String>> {
    value
        .as_array()
        .ok_or_else(|| format!("{label} must be an array"))?
        .iter()
        .map(|item| {
            item
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{label} must contain only strings").into())
        })
        .collect()
}

fn validate_model(model: &Model) -> Result<Model> {
    if model.fields.is_empty() {
        return Err("persistence model must contain fields".into());
    }
    let field_names: BTreeSet<&str> =
        model.fields.iter().map(|field| field.name.as_str()).collect();
    if field_names.len() != model.fields.len() {
        return Err("persistence field names must be unique".into());
    }
    if model.primary_key.is_empty() {
        return Err("persistence primary key must not be empty".into());
    }
    for field in &model.fields {
        if field.logical_type == "enum" {
            if field.enum_values.is_empty() {
                return Err(format!("enum field {} must not be empty", field.name).into());
            }
            let unique: BTreeSet<&str> =
                field.enum_values.iter().map(String::as_str).collect();
            if unique.len() != field.enum_values.len() {
                return Err(format!("enum field {} contains duplicate values", field.name).into());
            }
        }
    }
    for name in model
        .primary_key
        .iter()
        .chain(model.unique.iter().flatten())
    {
        if !field_names.contains(name.as_str()) {
            return Err(format!("constraint references unknown field {name}").into());
        }
    }
    Ok(model.clone())
}

fn canonical_model(model: &Model) -> Value {
    let columns: BTreeMap<&str, &str> = model
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.column.as_str()))
        .collect();
    json!({
        "name": model.name,
        "table": model.table,
        "primaryKey": model.primary_key.iter().map(|name| columns[name.as_str()]).collect::<Vec<_>>(),
        "unique": model.unique.iter().map(|group| group.iter().map(|name| columns[name.as_str()]).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "fields": model.fields,
    })
}

fn sql_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn render_sql(model: &Model) -> String {
    let columns: BTreeMap<&str, &str> = model
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field.column.as_str()))
        .collect();
    let mut lines = model
        .fields
        .iter()
        .map(|field| {
            format!(
                "  {} {}{}",
                sql_identifier(&field.column),
                field.sql_type,
                if field.nullable { "" } else { " NOT NULL" }
            )
        })
        .collect::<Vec<_>>();
    let primary_columns = model
        .primary_key
        .iter()
        .map(|name| sql_identifier(columns[name.as_str()]))
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!(
        "  CONSTRAINT {} PRIMARY KEY ({primary_columns})",
        sql_identifier(&format!("pk_{}", model.table))
    ));
    for group in &model.unique {
        let names = group
            .iter()
            .map(|name| columns[name.as_str()])
            .collect::<Vec<_>>();
        lines.push(format!(
            "  CONSTRAINT {} UNIQUE ({})",
            sql_identifier(&format!(
                "uq_{}_{}",
                model.table,
                names.join("_")
            )),
            names
                .iter()
                .map(|name| sql_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for field in &model.fields {
        if field.enum_values.is_empty() {
            continue;
        }
        lines.push(format!(
            "  CONSTRAINT {} CHECK ({} IN ({}))",
            sql_identifier(&format!("ck_{}_{}", model.table, field.column)),
            sql_identifier(&field.column),
            field
                .enum_values
                .iter()
                .map(|value| sql_literal(value))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    format!(
        "CREATE TABLE {} (\n{}\n);\n",
        sql_identifier(&model.table),
        lines.join(",\n")
    )
}

fn typescript_type(field: &Field) -> String {
    match field.logical_type.as_str() {
        "int32" => "number".to_owned(),
        "enum" => field
            .enum_values
            .iter()
            .map(|value| serde_json::to_string(value).expect("string serializes"))
            .collect::<Vec<_>>()
            .join(" | "),
        _ => "string".to_owned(),
    }
}

fn render_typescript_declarations(model: &Model) -> String {
    let status = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(typescript_type)
        .unwrap_or_else(|| "never".to_owned());
    let fields = model
        .fields
        .iter()
        .map(|field| {
            format!(
                "  readonly {}{}: {};",
                field.name,
                if field.nullable { "?" } else { "" },
                typescript_type(field)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "export type IdempotencyStatus = {status};\n\nexport interface {} {{\n{fields}\n}}\n",
        model.name
    )
}

fn javascript_predicate(field: &Field) -> String {
    let value = format!("value.{}", field.name);
    let base = match field.logical_type.as_str() {
        "int32" => format!(
            "Number.isInteger({value}) && {value} >= -2147483648 && {value} <= 2147483647"
        ),
        "datetime" => format!(
            "typeof {value} === \"string\" && !Number.isNaN(Date.parse({value}))"
        ),
        "enum" => format!("idempotencyStatusSet.has({value})"),
        _ => format!("typeof {value} === \"string\""),
    };
    if field.nullable {
        format!("({value} === undefined || ({base}))")
    } else {
        base
    }
}

fn render_typescript_runtime(model: &Model) -> String {
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    let allowed = model
        .fields
        .iter()
        .map(|field| serde_json::to_string(&field.name).expect("string serializes"))
        .collect::<Vec<_>>()
        .join(", ");
    let predicates = model
        .fields
        .iter()
        .map(javascript_predicate)
        .collect::<Vec<_>>()
        .join(" &&\n    ");
    format!(
        "export const idempotencyStatuses = Object.freeze({});\nconst idempotencyStatusSet = new Set(idempotencyStatuses);\nconst allowedFields = new Set([{allowed}]);\n\nexport function isIdempotencyRecord(value) {{\n  if (value === null || typeof value !== \"object\" || Array.isArray(value)) {{\n    return false;\n  }}\n  if (Object.keys(value).some((key) => !allowedFields.has(key))) {{\n    return false;\n  }}\n  return (\n    {predicates}\n  );\n}}\n",
        serde_json::to_string(&enum_values).expect("enum serializes")
    )
}

fn rust_type(field: &Field) -> &'static str {
    match (field.logical_type.as_str(), field.nullable) {
        ("int32", false) => "i32",
        ("int32", true) => "Option<i32>",
        ("enum", false) => "IdempotencyStatus",
        ("enum", true) => "Option<IdempotencyStatus>",
        (_, false) => "String",
        (_, true) => "Option<String>",
    }
}

fn render_rust(model: &Model) -> String {
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    let variants = enum_values
        .iter()
        .map(|value| format!("    {},", pascal_case(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let as_str_arms = enum_values
        .iter()
        .map(|value| {
            format!(
                "            Self::{} => \"{}\",",
                pascal_case(value),
                value
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let matches = enum_values
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let fields = model
        .fields
        .iter()
        .map(|field| format!("    pub {}: {},", field.column, rust_type(field)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "#![forbid(unsafe_code)]\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum IdempotencyStatus {{\n{variants}\n}}\n\nimpl IdempotencyStatus {{\n    pub const fn as_str(self) -> &'static str {{\n        match self {{\n{as_str_arms}\n        }}\n    }}\n}}\n\npub fn is_valid_idempotency_status(value: &str) -> bool {{\n    matches!(value, {matches})\n}}\n\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct {} {{\n{fields}\n}}\n",
        model.name
    )
}

fn go_type(field: &Field) -> &'static str {
    match (field.logical_type.as_str(), field.nullable) {
        ("int32", false) => "int32",
        ("int32", true) => "*int32",
        ("enum", false) => "IdempotencyStatus",
        ("enum", true) => "*IdempotencyStatus",
        (_, false) => "string",
        (_, true) => "*string",
    }
}

fn render_go(model: &Model) -> String {
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    let constants = enum_values
        .iter()
        .map(|value| {
            format!(
                "\tIdempotencyStatus{} IdempotencyStatus = \"{}\"",
                pascal_case(value),
                value
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cases = enum_values
        .iter()
        .map(|value| format!("IdempotencyStatus{}", pascal_case(value)))
        .collect::<Vec<_>>()
        .join(",\n\t\t");
    let fields = model
        .fields
        .iter()
        .map(|field| {
            format!(
                "\t{} {} `json:\"{}{}\"`",
                pascal_case(&field.name),
                go_type(field),
                field.name,
                if field.nullable { ",omitempty" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "package persistence\n\ntype IdempotencyStatus string\n\nconst (\n{constants}\n)\n\nfunc (value IdempotencyStatus) Valid() bool {{\n\tswitch value {{\n\tcase {cases}:\n\t\treturn true\n\tdefault:\n\t\treturn false\n\t}}\n}}\n\ntype {} struct {{\n{fields}\n}}\n",
        model.name
    )
}

fn gleam_type(field: &Field) -> &'static str {
    match (field.logical_type.as_str(), field.nullable) {
        ("int32", false) => "Int",
        ("int32", true) => "Option(Int)",
        ("enum", false) => "IdempotencyStatus",
        ("enum", true) => "Option(IdempotencyStatus)",
        (_, false) => "String",
        (_, true) => "Option(String)",
    }
}

fn render_gleam(model: &Model) -> String {
    let fields = model
        .fields
        .iter()
        .map(|field| format!("    {}: {},", field.column, gleam_type(field)))
        .collect::<Vec<_>>()
        .join("\n");
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    let variants = enum_values
        .iter()
        .map(|value| format!("  {}", pascal_case(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let cases = enum_values
        .iter()
        .map(|value| {
            format!(
                "    \"{}\" -> Ok({})",
                value,
                pascal_case(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "import gleam/option.{{type Option}}\n\npub type IdempotencyStatus {{\n{variants}\n}}\n\npub type {} {{\n  {}(\n{fields}\n  )\n}}\n\npub fn idempotency_status_from_string(value: String) -> Result(IdempotencyStatus, Nil) {{\n  case value {{\n{cases}\n    _ -> Error(Nil)\n  }}\n}}\n",
        model.name,
        model.name
    )
}

fn elixir_type(field: &Field) -> &'static str {
    match field.logical_type.as_str() {
        "int32" => "integer()",
        _ => "String.t()",
    }
}

fn render_elixir(model: &Model) -> String {
    let required = model
        .fields
        .iter()
        .filter(|field| !field.nullable)
        .map(|field| format!(":{}", field.column))
        .collect::<Vec<_>>()
        .join(", ");
    let all = model
        .fields
        .iter()
        .map(|field| format!(":{}", field.column))
        .collect::<Vec<_>>()
        .join(", ");
    let fields = model
        .fields
        .iter()
        .map(|field| {
            format!(
                "          {}: {}{}",
                field.column,
                elixir_type(field),
                if field.nullable { " | nil" } else { "" }
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    format!(
        "defmodule OresMiddleware.Generated.{} do\n  @moduledoc false\n  @enforce_keys [{required}]\n  defstruct [{all}]\n\n  @type t :: %__MODULE__{{\n{fields}\n        }}\n\n  @idempotency_statuses {}\n  @spec valid_idempotency_status?(term()) :: boolean()\n  def valid_idempotency_status?(value), do: value in @idempotency_statuses\nend\n",
        model.name,
        serde_json::to_string(&enum_values).expect("enum serializes")
    )
}

fn erlang_type(field: &Field) -> String {
    let base = match field.logical_type.as_str() {
        "int32" => "integer()",
        "enum" => "idempotency_status()",
        _ => "binary()",
    };
    format!(
        "{} {} {}",
        field.column,
        if field.nullable { "=>" } else { ":=" },
        base
    )
}

fn render_erlang(model: &Model) -> String {
    let fields = model
        .fields
        .iter()
        .map(erlang_type)
        .collect::<Vec<_>>()
        .join(",\n    ");
    let enum_values = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum")
        .map(|field| field.enum_values.clone())
        .unwrap_or_default();
    let status_type = enum_values
        .iter()
        .map(|value| format!("<<\"{value}\">>"))
        .collect::<Vec<_>>()
        .join(" | ");
    let clauses = enum_values
        .iter()
        .map(|value| format!("valid_idempotency_status(<<\"{value}\">>) -> true;"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "-module(ores_middleware_generated_idempotency_record).\n-export([valid_idempotency_status/1]).\n-export_type([idempotency_record/0, idempotency_status/0]).\n\n-type idempotency_status() :: {status_type}.\n-type idempotency_record() :: #{{\n    {fields}\n}}.\n\n-spec valid_idempotency_status(term()) -> boolean().\n{clauses}\nvalid_idempotency_status(_) -> false.\n"
    )
}

fn diesel_type(field: &Field) -> &'static str {
    match (field.logical_type.as_str(), field.nullable) {
        ("int32", false) => "i32",
        ("int32", true) => "Option<i32>",
        ("datetime", false) => "chrono::DateTime<chrono::Utc>",
        ("datetime", true) => "Option<chrono::DateTime<chrono::Utc>>",
        (_, false) => "String",
        (_, true) => "Option<String>",
    }
}

fn render_diesel(model: &Model) -> String {
    let fields = model
        .fields
        .iter()
        .map(|field| format!("    pub {}: {},", field.column, diesel_type(field)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "#[derive(Clone, Debug, diesel::Queryable, diesel::Selectable, diesel::Insertable, diesel::Identifiable)]\n#[diesel(table_name = {})]\n#[diesel(primary_key({}))]\npub struct {} {{\n{fields}\n}}\n",
        model.table,
        model
            .primary_key
            .iter()
            .map(|name| snake_case(name))
            .collect::<Vec<_>>()
            .join(", "),
        model.name
    )
}

fn seaorm_type(field: &Field) -> &'static str {
    match (field.logical_type.as_str(), field.nullable) {
        ("int32", false) => "i32",
        ("int32", true) => "Option<i32>",
        ("datetime", false) => "DateTimeUtc",
        ("datetime", true) => "Option<DateTimeUtc>",
        (_, false) => "String",
        (_, true) => "Option<String>",
    }
}

fn render_seaorm(model: &Model) -> String {
    let primary: BTreeSet<&str> =
        model.primary_key.iter().map(String::as_str).collect();
    let fields = model
        .fields
        .iter()
        .map(|field| {
            let attribute = if primary.contains(field.name.as_str()) {
                "    #[sea_orm(primary_key, auto_increment = false)]\n"
            } else {
                ""
            };
            format!(
                "{attribute}    pub {}: {},",
                field.column,
                seaorm_type(field)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "use sea_orm::entity::prelude::*;\n\n#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]\n#[sea_orm(table_name = \"{}\")]\npub struct Model {{\n{fields}\n}}\n\n#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]\npub enum Relation {{}}\n\nimpl ActiveModelBehavior for ActiveModel {{}}\n",
        model.table
    )
}

fn render_proto(model: &Model) -> String {
    let enum_field = model
        .fields
        .iter()
        .find(|field| field.logical_type == "enum");
    let enum_block = enum_field.map_or_else(String::new, |field| {
        let values = field
            .enum_values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!(
                    "  IDEMPOTENCY_STATUS_{} = {};",
                    value.to_ascii_uppercase(),
                    index + 1
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "enum IdempotencyStatus {{\n  IDEMPOTENCY_STATUS_UNSPECIFIED = 0;\n{values}\n}}\n\n"
        )
    });
    let proto_fields = model
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value_type = match field.logical_type.as_str() {
                "int32" => "int32",
                "datetime" => "google.protobuf.Timestamp",
                "enum" => "IdempotencyStatus",
                _ => "string",
            };
            format!(
                "  {}{} {} = {};",
                if field.nullable { "optional " } else { "" },
                value_type,
                field.column,
                index + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "syntax = \"proto3\";\n\npackage ores.middleware.persistence.v1;\n\nimport \"google/protobuf/timestamp.proto\";\n\n{enum_block}message {} {{\n{proto_fields}\n}}\n",
        model.name
    )
}

fn render_grpc_manifest(model: &Model) -> Value {
    json!({
        "schema": "ores.grpc-projection/v1",
        "authority": "typespec",
        "messages": [model.name],
        "services": [],
        "operations": [],
        "messagesOnly": true,
        "reason": "the persistence authority declares a data model but no RPC operations"
    })
}

fn json_schema_component(root: &Path) -> Result<Value> {
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join(JSON_SCHEMA_SOURCE))?)?;
    Ok(schema)
}

fn render_openapi(root: &Path) -> Result<Value> {
    let schema = json_schema_component(root)?;
    let definitions = schema
        .get("$defs")
        .cloned()
        .ok_or("JSON Schema must contain $defs")?;
    Ok(json!({
        "openapi": "3.1.1",
        "info": {
            "title": "ORES middleware persistence components",
            "version": "1.0.0"
        },
        "paths": {},
        "components": {"schemas": definitions},
        "x-ores-authority": "json-schema-openapi",
        "x-ores-no-invented-operations": true
    }))
}

fn lane_artifacts(
    authority: Authority,
    model: &Model,
    root: &Path,
) -> Result<BTreeMap<String, String>> {
    let mut artifacts = BTreeMap::from([
        (
            "model.json".to_owned(),
            format!(
                "{}\n",
                serde_json::to_string_pretty(&canonical_model(model))?
            ),
        ),
        ("sql/schema.sql".to_owned(), render_sql(model)),
        (
            "typescript/idempotency_record.d.ts".to_owned(),
            render_typescript_declarations(model),
        ),
        (
            "typescript/idempotency_record.mjs".to_owned(),
            render_typescript_runtime(model),
        ),
        (
            "rust/idempotency_record.rs".to_owned(),
            render_rust(model),
        ),
        (
            "golang/idempotency_record.go".to_owned(),
            render_go(model),
        ),
        (
            "gleam/idempotency_record.gleam".to_owned(),
            render_gleam(model),
        ),
        (
            "elixir/idempotency_record.ex".to_owned(),
            render_elixir(model),
        ),
        (
            "erlang/idempotency_record.erl".to_owned(),
            render_erlang(model),
        ),
        (
            "diesel/idempotency_record.rs".to_owned(),
            render_diesel(model),
        ),
        (
            "seaorm/idempotency_record.rs".to_owned(),
            render_seaorm(model),
        ),
    ]);
    match authority {
        Authority::TypeSpec => {
            artifacts.insert(
                "protobuf/idempotency_record.proto".to_owned(),
                render_proto(model),
            );
            artifacts.insert(
                "grpc/projection.json".to_owned(),
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&render_grpc_manifest(model))?
                ),
            );
        }
        Authority::JsonSchemaOpenApi => {
            artifacts.insert(
                "openapi/idempotency_record.openapi.json".to_owned(),
                format!("{}\n", serde_json::to_string_pretty(&render_openapi(root)?)?),
            );
            artifacts.insert(
                "http/write-client.json".to_owned(),
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&json!({
                        "schema": "ores.http-write-client-projection/v1",
                        "authority": "json-schema-openapi",
                        "models": [model.name],
                        "operations": [],
                        "messagesOnly": true,
                        "reason": "the persistence authority declares a data model but no HTTP operations"
                    }))?
                ),
            );
        }
    }
    Ok(artifacts)
}

fn generate_lane(authority: Authority, root: &Path) -> Result<LaneOutput> {
    let model = match authority {
        Authority::TypeSpec => parse_typespec(root)?,
        Authority::JsonSchemaOpenApi => parse_json_schema(root)?,
    };
    let artifacts = lane_artifacts(authority, &model, root)?;
    Ok(LaneOutput {
        authority,
        model,
        artifacts,
    })
}

fn compare_lanes(left: &LaneOutput, right: &LaneOutput) -> Vec<Discrepancy> {
    let mut findings = Vec::new();
    if canonical_model(&left.model) != canonical_model(&right.model) {
        findings.push(Discrepancy::new(
            "peer-contract-model-mismatch",
            format!(
                "TypeSpec={} JSON-Schema={}",
                canonical_model(&left.model),
                canonical_model(&right.model)
            ),
        ));
    }
    for path in COMMON_ARTIFACTS {
        match (left.artifacts.get(*path), right.artifacts.get(*path)) {
            (Some(left_content), Some(right_content))
                if left_content == right_content => {}
            (Some(left_content), Some(right_content)) => {
                findings.push(Discrepancy::new(
                    "generated-artifact-mismatch",
                    format!(
                        "{path}: TypeSpec sha256={} JSON-Schema sha256={}",
                        sha256_bytes(left_content.as_bytes()),
                        sha256_bytes(right_content.as_bytes())
                    ),
                ));
            }
            _ => findings.push(Discrepancy::new(
                "missing-generated-artifact",
                format!("both authorities must generate {path}"),
            )),
        }
    }
    findings
}

fn write_lane(
    root: &Path,
    output_root: &Path,
    lane: &LaneOutput,
) -> Result<LaneReceipt> {
    let folder = output_root.join(lane.authority.output_folder());
    let mut receipts = Vec::new();
    for (relative, content) in &lane.artifacts {
        let path = folder.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        receipts.push(ArtifactReceipt {
            path: path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/"),
            sha256: sha256_bytes(content.as_bytes()),
            bytes: content.len(),
        });
    }
    Ok(LaneReceipt {
        authority: lane.authority.id().to_owned(),
        source: lane.authority.source().to_owned(),
        source_sha256: sha256_bytes(&fs::read(root.join(lane.authority.source()))?),
        output_root: folder
            .strip_prefix(root)
            .unwrap_or(&folder)
            .to_string_lossy()
            .replace('\\', "/"),
        artifacts: receipts,
    })
}

fn write_report(
    path: &Path,
    lanes: Vec<LaneReceipt>,
    findings: &[Discrepancy],
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = json!({
        "schema": "ores.persistence-polyglot-convergence/v2",
        "authorities": ["typespec", "json-schema-openapi"],
        "flows": {
            "typespec": ["sql", "types-interfaces", "runtime-code", "diesel", "seaorm", "protobuf", "grpc", "wire-clients"],
            "json-schema-openapi": ["sql", "types-interfaces", "runtime-code", "diesel", "seaorm", "openapi", "http-write-clients"]
        },
        "commonPolyglotTargets": COMMON_ARTIFACTS,
        "lanes": lanes,
        "status": if findings.is_empty() { "passed" } else { "stopped_for_evaluation" },
        "zeroUnexplainedFindings": findings.is_empty(),
        "discrepancies": findings,
    });
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&report)?))?;
    Ok(())
}

fn run(root: &Path, output_root: &Path, report: &Path) -> Result<Vec<Discrepancy>> {
    let typespec = generate_lane(Authority::TypeSpec, root)?;
    let json_schema = generate_lane(Authority::JsonSchemaOpenApi, root)?;
    let findings = compare_lanes(&typespec, &json_schema);
    let receipts = vec![
        write_lane(root, output_root, &typespec)?,
        write_lane(root, output_root, &json_schema)?,
    ];
    write_report(report, receipts, &findings)?;
    Ok(findings)
}

fn resolve(root: &Path, value: PathBuf) -> PathBuf {
    if value.is_absolute() {
        value
    } else {
        root.join(value)
    }
}

fn parse_args() -> std::result::Result<(PathBuf, PathBuf, PathBuf), String> {
    let mut root = env::current_dir().map_err(|error| error.to_string())?;
    let mut output = PathBuf::from("target/schema-convergence");
    let mut report = None;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => {
                root = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--root requires a path".to_owned())?,
                );
            }
            "--output-root" => {
                output = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output-root requires a path".to_owned())?,
                );
            }
            "--report" => {
                report = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--report requires a path".to_owned())?,
                ));
            }
            "-h" | "--help" => {
                println!(
                    "Usage: persistence_codegen [--root PATH] [--output-root PATH] [--report PATH]\n\
                     Generate independent TypeSpec and JSON Schema polyglot lanes and compare them."
                );
                return Err(String::new());
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    root = root
        .canonicalize()
        .map_err(|error| format!("invalid root {}: {error}", root.display()))?;
    let output = resolve(&root, output);
    let report = resolve(
        &root,
        report.unwrap_or_else(|| output.join("receipt.json")),
    );
    Ok((root, output, report))
}

fn main() -> ExitCode {
    let (root, output, report) = match parse_args() {
        Ok(arguments) => arguments,
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(64);
        }
    };
    match run(&root, &output, &report) {
        Ok(findings) if findings.is_empty() => {
            println!(
                "independent TypeSpec and JSON Schema polyglot generation passed; report={}",
                report.display()
            );
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            println!(
                "STOPPED_FOR_EVALUATION: {} discrepancy(s); report={}",
                findings.len(),
                report.display()
            );
            for finding in findings {
                println!(
                    "- {}: {}: {}",
                    finding.fingerprint, finding.kind, finding.detail
                );
            }
            ExitCode::from(2)
        }
        Err(error) => {
            let finding = Discrepancy::new(
                "polyglot-generator-failure",
                format!("{}: {error}", std::any::type_name_of_val(&error)),
            );
            let _ = write_report(&report, Vec::new(), std::slice::from_ref(&finding));
            eprintln!(
                "STOPPED_FOR_EVALUATION: {}: {}; report={}",
                finding.kind,
                finding.detail,
                report.display()
            );
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn fixture_root() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        let persistence = temp.path().join("contracts/persistence");
        fs::create_dir_all(&persistence).expect("create persistence folder");
        for relative in [TYPESPEC_SOURCE, JSON_SCHEMA_SOURCE] {
            let source = repository_root().join(relative);
            let destination = temp.path().join(relative);
            fs::copy(source, destination).expect("copy authority");
        }
        temp
    }

    fn check(root: &Path) -> Vec<Discrepancy> {
        let output = root.join("target/schema-convergence");
        let report = output.join("receipt.json");
        run(root, &output, &report).expect("convergence run")
    }

    #[test]
    fn current_authorities_generate_equal_polyglot_artifacts() {
        let temp = fixture_root();
        assert_eq!(check(temp.path()), Vec::new());
        for authority in [Authority::TypeSpec, Authority::JsonSchemaOpenApi] {
            for relative in COMMON_ARTIFACTS {
                assert!(
                    temp.path()
                        .join("target/schema-convergence")
                        .join(authority.output_folder())
                        .join(relative)
                        .is_file(),
                    "missing {} artifact {relative}",
                    authority.id()
                );
            }
        }
    }

    #[test]
    fn each_lane_generates_both_orms() {
        let temp = fixture_root();
        assert_eq!(check(temp.path()), Vec::new());
        for authority in [Authority::TypeSpec, Authority::JsonSchemaOpenApi] {
            let lane = temp
                .path()
                .join("target/schema-convergence")
                .join(authority.output_folder());
            assert!(lane.join("diesel/idempotency_record.rs").is_file());
            assert!(lane.join("seaorm/idempotency_record.rs").is_file());
        }
    }

    #[test]
    fn authority_specific_transport_outputs_do_not_invent_operations() {
        let temp = fixture_root();
        assert_eq!(check(temp.path()), Vec::new());
        let output = temp.path().join("target/schema-convergence");
        let grpc: Value = serde_json::from_slice(
            &fs::read(output.join("typespec/grpc/projection.json")).unwrap(),
        )
        .unwrap();
        let openapi: Value = serde_json::from_slice(
            &fs::read(
                output.join(
                    "json-schema-openapi/openapi/idempotency_record.openapi.json",
                ),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(grpc["messagesOnly"], true);
        assert_eq!(grpc["operations"], json!([]));
        assert_eq!(openapi["paths"], json!({}));
        assert_eq!(openapi["x-ores-no-invented-operations"], true);
    }

    #[test]
    fn requiredness_drift_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join(JSON_SCHEMA_SOURCE);
        let mut schema: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["$defs"]["IdempotencyRecord"]["required"]
            .as_array_mut()
            .unwrap()
            .push(json!("responseBody"));
        fs::write(&path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let findings = check(temp.path());
        assert!(findings.iter().any(|finding| {
            finding.kind == "peer-contract-model-mismatch"
                || finding.kind == "generated-artifact-mismatch"
        }));
    }

    #[test]
    fn enum_drift_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join(JSON_SCHEMA_SOURCE);
        let mut schema: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["$defs"]["IdempotencyStatus"]["enum"]
            .as_array_mut()
            .unwrap()
            .pop();
        fs::write(&path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let findings = check(temp.path());
        assert!(findings.iter().any(|finding| {
            finding.kind == "peer-contract-model-mismatch"
                || finding.kind == "generated-artifact-mismatch"
        }));
    }

    #[test]
    fn sql_table_drift_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join(JSON_SCHEMA_SOURCE);
        let mut schema: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["$defs"]["IdempotencyRecord"]["x-ores-sql"]["table"] =
            json!("wrong_table");
        fs::write(&path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let findings = check(temp.path());
        assert!(findings.iter().any(|finding| {
            finding.kind == "peer-contract-model-mismatch"
                || finding.kind == "generated-artifact-mismatch"
        }));
    }

    #[test]
    fn receipt_binds_both_source_digests_and_every_common_artifact() {
        let temp = fixture_root();
        assert_eq!(check(temp.path()), Vec::new());
        let report: Value = serde_json::from_slice(
            &fs::read(temp.path().join("target/schema-convergence/receipt.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(report["status"], "passed");
        assert_eq!(report["zeroUnexplainedFindings"], true);
        assert_eq!(report["lanes"].as_array().unwrap().len(), 2);
        for lane in report["lanes"].as_array().unwrap() {
            assert_eq!(lane["sourceSha256"].as_str().unwrap().len(), 64);
            assert!(
                lane["artifacts"].as_array().unwrap().len()
                    >= COMMON_ARTIFACTS.len()
            );
        }
    }
}
