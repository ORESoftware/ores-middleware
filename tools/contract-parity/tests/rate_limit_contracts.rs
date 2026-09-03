use regex::Regex;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn extract_block(source: &str, keyword: &str, name: &str) -> String {
    let expression = format!(
        r"(?ms)\b{}\s+{}\s*\{{(?P<body>.*?)^\s*\}}",
        regex::escape(keyword),
        regex::escape(name)
    );
    Regex::new(&expression)
        .expect("valid block expression")
        .captures(source)
        .and_then(|captures| captures.name("body"))
        .map(|body| body.as_str().to_owned())
        .unwrap_or_else(|| panic!("missing TypeSpec {keyword} {name}"))
}

fn typespec_enum(source: &str, name: &str) -> BTreeSet<String> {
    let body = extract_block(source, "enum", name);
    let member = Regex::new(r#"^[A-Za-z_][A-Za-z0-9_]*\s*:\s*"([^"]+)"\s*,?$"#)
        .expect("valid enum expression");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .map(|line| {
            member
                .captures(line)
                .unwrap_or_else(|| panic!("unsupported TypeSpec enum member in {name}: {line}"))[1]
                .to_owned()
        })
        .collect()
}

fn schema_enum(schema: &Value, definition: &str) -> BTreeSet<String> {
    schema
        .pointer(&format!("/$defs/{definition}/enum"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing JSON Schema enum {definition}"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("non-string JSON Schema enum value in {definition}"))
                .to_owned()
        })
        .collect()
}

fn typespec_model_signature(source: &str, name: &str) -> BTreeMap<String, bool> {
    let body = extract_block(source, "model", name);
    let property = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*[^;]+;$")
        .expect("valid property expression");
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('*'))
        .filter(|line| !line.starts_with('@'))
        .map(|line| {
            let captures = property
                .captures(line)
                .unwrap_or_else(|| panic!("unsupported TypeSpec property in {name}: {line}"));
            (captures[1].to_owned(), captures.get(2).is_none())
        })
        .collect()
}

fn schema_model_signature(schema: &Value, definition: &str) -> BTreeMap<String, bool> {
    let model = schema
        .pointer(&format!("/$defs/{definition}"))
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("missing JSON Schema model {definition}"));
    let required: BTreeSet<&str> = model
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    model
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("missing JSON Schema properties for {definition}"))
        .keys()
        .map(|name| (name.clone(), required.contains(name.as_str())))
        .collect()
}

#[test]
fn layered_rate_limit_enums_match_between_peer_authorities() {
    let root = repository_root();
    let typespec = fs::read_to_string(root.join("contracts/typespec/main.tsp"))
        .expect("read TypeSpec authority");
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("contracts/json-schema/middleware-stack.schema.json"))
            .expect("read JSON Schema authority"),
    )
    .expect("parse JSON Schema authority");

    for (typespec_name, schema_name) in [
        ("RateLimitKey", "rateLimitSignal"),
        ("RateLimitAlgorithm", "rateLimitAlgorithm"),
        ("RateLimitLayer", "rateLimitLayer"),
        ("RateLimitFailureMode", "rateLimitFailureMode"),
        (
            "RateLimitKeyDerivationMode",
            "rateLimitKeyDerivationMode",
        ),
        ("RateLimitDecisionKind", "rateLimitDecisionKind"),
        ("RateLimitDecisionSource", "rateLimitDecisionSource"),
    ] {
        assert_eq!(
            typespec_enum(&typespec, typespec_name),
            schema_enum(&schema, schema_name),
            "peer-authority enum drift: {typespec_name} vs {schema_name}"
        );
    }
}

#[test]
fn layered_rate_limit_model_signatures_match_between_peer_authorities() {
    let root = repository_root();
    let typespec = fs::read_to_string(root.join("contracts/typespec/main.tsp"))
        .expect("read TypeSpec authority");
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("contracts/json-schema/middleware-stack.schema.json"))
            .expect("read JSON Schema authority"),
    )
    .expect("parse JSON Schema authority");

    for (typespec_name, schema_name) in [
        ("RateLimitPolicy", "rateLimit"),
        ("RateLimitPrincipal", "rateLimitPrincipal"),
        ("RateLimitDecision", "rateLimitDecision"),
    ] {
        assert_eq!(
            typespec_model_signature(&typespec, typespec_name),
            schema_model_signature(&schema, schema_name),
            "peer-authority model drift: {typespec_name} vs {schema_name}"
        );
    }
}

#[test]
fn privacy_and_memory_bounds_remain_fail_closed() {
    let root = repository_root();
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("contracts/json-schema/middleware-stack.schema.json"))
            .expect("read JSON Schema authority"),
    )
    .expect("parse JSON Schema authority");

    assert_eq!(
        schema.pointer("/$defs/rateLimit/properties/localCacheMaxEntries/maximum"),
        Some(&Value::from(10_000))
    );
    assert_eq!(
        schema.pointer("/$defs/rateLimitPrincipal/properties/digest/pattern"),
        Some(&Value::from("^[0-9a-f]{64}$"))
    );

    let edge_items = schema
        .pointer("/$defs/rateLimit/allOf/0/then/properties/keyBy/items/enum")
        .and_then(Value::as_array)
        .expect("Cloudflare edge key allowlist");
    let edge_items: BTreeSet<&str> = edge_items.iter().filter_map(Value::as_str).collect();
    assert_eq!(
        edge_items,
        BTreeSet::from(["ip", "ip-prefix", "method", "route"])
    );
}
