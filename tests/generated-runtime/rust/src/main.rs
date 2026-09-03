#![forbid(unsafe_code)]

mod generated;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{Error, ErrorKind};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const WITNESS_SCHEMA: &str = "ores.generated-runtime-witness/v1";
const GENERATED_SOURCE: &str = include_str!("generated.rs");
const USAGE: &str = "usage: generated-rust-witness <fixture.json> <authority>";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    model: String,
    wire_fields: Vec<String>,
    required_fields: Vec<String>,
    optional_fields: Vec<String>,
    statuses: Vec<String>,
    cases: Vec<TestCase>,
}

#[derive(Debug, Deserialize)]
struct TestCase {
    id: String,
    #[serde(rename = "expect")]
    _expect: String,
    value: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    id: String,
    accepted: bool,
    normalized: Option<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Witness {
    schema: &'static str,
    authority: String,
    language: &'static str,
    model: String,
    wire_fields: Vec<String>,
    required_fields: Vec<String>,
    optional_fields: Vec<String>,
    statuses: Vec<String>,
    status_acceptance: BTreeMap<String, bool>,
    cases: Vec<CaseResult>,
}

fn source_shape_guard() -> Result<(), String> {
    let required_fragments = [
        "pub created_at: String,",
        "pub expires_at: String,",
        "pub id: String,",
        "pub idempotency_key: String,",
        "pub request_hash: String,",
        "pub response_body: Option<String>,",
        "pub response_status: Option<i32>,",
        "pub status: IdempotencyStatus,",
        "pub tenant_id: String,",
        "Pending,",
        "Succeeded,",
        "Failed,",
    ];
    for fragment in required_fragments {
        if !GENERATED_SOURCE.contains(fragment) {
            return Err(format!(
                "generated Rust source is missing required fragment: {fragment}"
            ));
        }
    }
    Ok(())
}

fn required_string(object: &Map<String, Value>, name: &str) -> Option<String> {
    object.get(name)?.as_str().map(str::to_owned)
}

fn optional_string(object: &Map<String, Value>, name: &str) -> Option<Option<String>> {
    match object.get(name) {
        None => Some(None),
        Some(Value::String(value)) => Some(Some(value.clone())),
        Some(_) => None,
    }
}

fn optional_i32(object: &Map<String, Value>, name: &str) -> Option<Option<i32>> {
    match object.get(name) {
        None => Some(None),
        Some(value) => {
            let integer = value.as_i64()?;
            i32::try_from(integer).ok().map(Some)
        }
    }
}

fn status_from_wire(value: &str) -> Option<generated::IdempotencyStatus> {
    if !generated::is_valid_idempotency_status(value) {
        return None;
    }
    match value {
        "pending" => Some(generated::IdempotencyStatus::Pending),
        "succeeded" => Some(generated::IdempotencyStatus::Succeeded),
        "failed" => Some(generated::IdempotencyStatus::Failed),
        _ => None,
    }
}

fn strict_decode(value: &Value, fixture: &Fixture) -> Option<generated::IdempotencyRecord> {
    let object = value.as_object()?;
    let allowed: BTreeSet<&str> = fixture.wire_fields.iter().map(String::as_str).collect();
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return None;
    }
    for name in &fixture.required_fields {
        if required_string(object, name).is_none() {
            return None;
        }
    }
    for name in &fixture.optional_fields {
        if object.get(name).is_some_and(Value::is_null) {
            return None;
        }
    }

    let created_at = required_string(object, "createdAt")?;
    let expires_at = required_string(object, "expiresAt")?;
    if OffsetDateTime::parse(&created_at, &Rfc3339).is_err()
        || OffsetDateTime::parse(&expires_at, &Rfc3339).is_err()
    {
        return None;
    }
    let status = status_from_wire(&required_string(object, "status")?)?;

    Some(generated::IdempotencyRecord {
        created_at,
        expires_at,
        id: required_string(object, "id")?,
        idempotency_key: required_string(object, "idempotencyKey")?,
        request_hash: required_string(object, "requestHash")?,
        response_body: optional_string(object, "responseBody")??,
        response_status: optional_i32(object, "responseStatus")??,
        status,
        tenant_id: required_string(object, "tenantId")?,
    })
}

fn normalize(record: generated::IdempotencyRecord) -> Value {
    let mut object = Map::new();
    object.insert("createdAt".to_owned(), json!(record.created_at));
    object.insert("expiresAt".to_owned(), json!(record.expires_at));
    object.insert("id".to_owned(), json!(record.id));
    object.insert(
        "idempotencyKey".to_owned(),
        json!(record.idempotency_key),
    );
    object.insert("requestHash".to_owned(), json!(record.request_hash));
    if let Some(value) = record.response_body {
        object.insert("responseBody".to_owned(), json!(value));
    }
    if let Some(value) = record.response_status {
        object.insert("responseStatus".to_owned(), json!(value));
    }
    object.insert("status".to_owned(), json!(record.status.as_str()));
    object.insert("tenantId".to_owned(), json!(record.tenant_id));
    Value::Object(object)
}

fn load_fixture(path: &Path) -> Result<Fixture, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn invalid_input(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidInput, message.into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let fixture_path = arguments.next().ok_or_else(|| invalid_input(USAGE))?;
    let authority = arguments.next().ok_or_else(|| invalid_input(USAGE))?;
    if arguments.next().is_some() {
        return Err(invalid_input(USAGE).into());
    }

    if let Err(message) = source_shape_guard() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("Rust source-shape witness failed: {message}"),
        )
        .into());
    }
    let fixture = load_fixture(Path::new(&fixture_path))?;

    let mut cases = Vec::with_capacity(fixture.cases.len());
    for test_case in &fixture.cases {
        let record = strict_decode(&test_case.value, &fixture);
        let accepted = record.is_some();
        cases.push(CaseResult {
            id: test_case.id.clone(),
            accepted,
            normalized: record.map(normalize),
        });
    }

    let mut status_acceptance = BTreeMap::new();
    for status in &fixture.statuses {
        status_acceptance.insert(
            status.clone(),
            generated::is_valid_idempotency_status(status),
        );
    }
    status_acceptance.insert(
        "__unknown__".to_owned(),
        generated::is_valid_idempotency_status("__unknown__"),
    );

    let witness = Witness {
        schema: WITNESS_SCHEMA,
        authority,
        language: "rust",
        model: fixture.model,
        wire_fields: fixture.wire_fields,
        required_fields: fixture.required_fields,
        optional_fields: fixture.optional_fields,
        statuses: fixture.statuses,
        status_acceptance,
        cases,
    };
    println!("{}", serde_json::to_string(&witness)?);
    Ok(())
}
