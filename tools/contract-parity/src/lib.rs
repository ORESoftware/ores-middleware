use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discrepancy {
    pub fingerprint: String,
    pub kind: String,
    pub detail: String,
    pub owner: String,
    pub resolution_state: String,
}

impl Discrepancy {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>) -> Self {
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

fn extract_block(source: &str, keyword: &str, name: &str) -> Result<String> {
    let expression = format!(
        r"(?s)\b{}\s+{}\s*\{{(?P<body>.*?)\}}",
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

fn parse_tsp_enum(source: &str, name: &str) -> Result<Vec<String>> {
    let body = extract_block(source, "enum", name)?;
    let member = Regex::new(r#"^[A-Za-z_][A-Za-z0-9_]*\s*:\s*"([^"]+)"\s*,?$"#)?;
    let mut values = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let captures = member
            .captures(line)
            .ok_or_else(|| format!("unsupported TypeSpec enum member in {name}: {line}"))?;
        values.push(captures[1].to_owned());
    }
    Ok(values)
}

fn string_shape(pattern: Option<String>) -> Value {
    let mut shape = Map::new();
    shape.insert("type".to_owned(), Value::String("string".to_owned()));
    if let Some(pattern) = pattern {
        shape.insert("pattern".to_owned(), Value::String(pattern));
    }
    Value::Object(shape)
}

fn normalize_tsp_type(type_name: &str, pattern: Option<String>) -> Result<Value> {
    let type_name = type_name.trim();
    match type_name {
        "string" => Ok(string_shape(pattern)),
        "boolean" if pattern.is_none() => Ok(json!({"type": "boolean"})),
        "uint16" if pattern.is_none() => {
            Ok(json!({"type": "integer", "minimum": 0, "maximum": 65_535}))
        }
        "Record<string>" if pattern.is_none() => Ok(json!({
            "type": "object",
            "additionalProperties": {"type": "string"}
        })),
        _ if pattern.is_some() => {
            Err(format!("@pattern is only supported on string properties, got {type_name}").into())
        }
        _ if Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$")?.is_match(type_name) => {
            Ok(json!({"ref": type_name}))
        }
        _ => Err(format!("unsupported TypeSpec type: {type_name}").into()),
    }
}

fn parse_pattern_decorator(line: &str) -> Result<Option<String>> {
    let decorator = Regex::new(r#"^@pattern\(("(?:[^"\\]|\\.)*")\)\s*$"#)?;
    let Some(captures) = decorator.captures(line) else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str::<String>(&captures[1])?))
}

fn parse_tsp_model(source: &str, name: &str) -> Result<Value> {
    let body = extract_block(source, "model", name)?;
    let property = Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)(\?)?\s*:\s*([^;]+);$")?;
    let mut properties = Map::new();
    let mut pending_pattern = None;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if line.starts_with('@') {
            if pending_pattern.is_some() {
                return Err(
                    format!("multiple decorators before a property in model {name}").into(),
                );
            }
            pending_pattern = parse_pattern_decorator(line)?;
            if pending_pattern.is_none() {
                return Err(format!("unsupported TypeSpec decorator in {name}: {line}").into());
            }
            continue;
        }

        let captures = property
            .captures(line)
            .ok_or_else(|| format!("unsupported TypeSpec property in {name}: {line}"))?;
        let property_name = captures[1].to_owned();
        let required = captures.get(2).is_none();
        let shape = normalize_tsp_type(&captures[3], pending_pattern.take())?;
        properties.insert(
            property_name,
            json!({
                "required": required,
                "shape": shape
            }),
        );
    }

    if pending_pattern.is_some() {
        return Err(format!("dangling @pattern decorator in model {name}").into());
    }
    Ok(Value::Object(properties))
}

fn normalize_json_shape(value: &Value) -> Result<Value> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("JSON Schema property must be an object: {value}"))?;
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        return Ok(json!({
            "ref": reference.rsplit('/').next().unwrap_or(reference)
        }));
    }

    match object.get("type").and_then(Value::as_str) {
        Some("integer") => {
            let mut result = Map::new();
            result.insert("type".to_owned(), Value::String("integer".to_owned()));
            if let Some(minimum) = object.get("minimum") {
                result.insert("minimum".to_owned(), minimum.clone());
            }
            if let Some(maximum) = object.get("maximum") {
                result.insert("maximum".to_owned(), maximum.clone());
            }
            Ok(Value::Object(result))
        }
        Some("string") => Ok(string_shape(
            object
                .get("pattern")
                .and_then(Value::as_str)
                .map(str::to_owned),
        )),
        Some("boolean") => Ok(json!({"type": "boolean"})),
        Some("object") => {
            if let Some(additional) = object.get("additionalProperties") {
                if additional.is_object() {
                    return Ok(json!({
                        "type": "object",
                        "additionalProperties": normalize_json_shape(additional)?
                    }));
                }
            }
            Ok(json!({"type": "object"}))
        }
        _ => Err(format!("unsupported JSON Schema property shape: {value}").into()),
    }
}

fn parse_json_model(schema: &Value, name: &str) -> Result<Value> {
    let definition = schema
        .pointer(&format!("/$defs/{name}"))
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing JSON Schema definition {name}"))?;
    let required: BTreeSet<&str> = definition
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let source_properties = definition
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("missing properties for JSON Schema definition {name}"))?;
    let mut properties = Map::new();
    for (property_name, property_schema) in source_properties {
        properties.insert(
            property_name.clone(),
            json!({
                "required": required.contains(property_name.as_str()),
                "shape": normalize_json_shape(property_schema)?
            }),
        );
    }
    Ok(Value::Object(properties))
}

fn compare(label: &str, left: &Value, right: &Value, out: &mut Vec<Discrepancy>) {
    if left != right {
        out.push(Discrepancy::new(
            "peer-contract-mismatch",
            format!("{label}: TypeSpec={left}; JSON-Schema={right}"),
        ));
    }
}

fn check_topology(topology: &Value) -> Vec<Discrepancy> {
    let mut discrepancies = Vec::new();
    let expected_ids = BTreeSet::from(["json-schema-openapi", "typespec"]);
    let actual_ids: BTreeSet<&str> = topology
        .get("authorities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|authority| {
            authority.get("kind").and_then(Value::as_str) == Some("human-authored")
                && authority.get("topLevel").and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|authority| authority.get("id").and_then(Value::as_str))
        .collect();
    if actual_ids != expected_ids {
        discrepancies.push(Discrepancy::new(
            "authority-topology",
            format!(
                "top-level human-authored authorities must be {expected_ids:?}, got {actual_ids:?}"
            ),
        ));
    }

    let expected_flows = json!({
        "typespec": ["sql-when-applicable", "protobuf", "grpc", "wire-clients"],
        "json-schema-openapi": [
            "interfaces-types",
            "sql-when-applicable",
            "write-clients"
        ]
    });
    if topology.get("flows") != Some(&expected_flows) {
        discrepancies.push(Discrepancy::new(
            "authority-topology",
            "authority flows do not match the required peer TypeSpec and JSON Schema/OpenAPI lanes",
        ));
    }

    let expected_edges = BTreeSet::from([
        ("json-schema-openapi", "typespec"),
        ("typespec", "json-schema-openapi"),
    ]);
    let actual_edges: BTreeSet<(&str, &str)> = topology
        .get("prohibitedAuthorityEdges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|edge| {
            let edge = edge.as_array()?;
            Some((edge.first()?.as_str()?, edge.get(1)?.as_str()?))
        })
        .collect();
    if actual_edges != expected_edges {
        discrepancies.push(Discrepancy::new(
            "authority-topology",
            "both cross-authority precedence edges must be explicitly prohibited",
        ));
    }

    if topology
        .get("onUnexplainedMismatch")
        .and_then(Value::as_str)
        != Some("STOPPED_FOR_EVALUATION")
    {
        discrepancies.push(Discrepancy::new(
            "authority-topology",
            "unexplained mismatches must enter STOPPED_FOR_EVALUATION",
        ));
    }
    discrepancies
}

pub fn run(root: &Path) -> Result<Vec<Discrepancy>> {
    let tsp = fs::read_to_string(root.join("contracts/docs-serving.tsp"))?;
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join("contracts/docs-serving.schema.json"))?)?;
    let topology: Value =
        serde_json::from_slice(&fs::read(root.join("contracts/authority-topology.json"))?)?;

    let mut discrepancies = check_topology(&topology);
    for name in ["DocsRepresentation", "DocsAction"] {
        let left = serde_json::to_value(parse_tsp_enum(&tsp, name)?)?;
        let right = schema
            .pointer(&format!("/$defs/{name}/enum"))
            .cloned()
            .ok_or_else(|| format!("missing JSON Schema enum {name}"))?;
        compare(&format!("enum {name}"), &left, &right, &mut discrepancies);
    }
    for name in ["DocsRequest", "DocsDecision"] {
        compare(
            &format!("model {name}"),
            &parse_tsp_model(&tsp, name)?,
            &parse_json_model(&schema, name)?,
            &mut discrepancies,
        );
    }
    Ok(discrepancies)
}

pub fn write_report(path: &Path, discrepancies: &[Discrepancy]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = json!({
        "schema": "ores.contract-parity-report/v1",
        "authorities": ["typespec", "json-schema-openapi"],
        "status": if discrepancies.is_empty() {
            "passed"
        } else {
            "stopped_for_evaluation"
        },
        "zeroUnexplainedFindings": discrepancies.is_empty(),
        "discrepancies": discrepancies
    });
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn fixture_root() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("contracts")).expect("contracts directory");
        for name in [
            "docs-serving.tsp",
            "docs-serving.schema.json",
            "authority-topology.json",
        ] {
            fs::copy(
                repository_root().join("contracts").join(name),
                temp.path().join("contracts").join(name),
            )
            .expect("copy fixture");
        }
        temp
    }

    #[test]
    fn current_peer_contracts_match() {
        assert_eq!(run(&repository_root()).expect("parity run"), Vec::new());
    }

    #[test]
    fn pattern_drift_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join("contracts/docs-serving.schema.json");
        let mut schema: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["$defs"]["DocsRequest"]["properties"]["runtimeContractDigest"]
            .as_object_mut()
            .unwrap()
            .remove("pattern");
        fs::write(&path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let findings = run(temp.path()).expect("parity run");
        assert!(findings.iter().any(|item| {
            item.kind == "peer-contract-mismatch" && item.detail.contains("runtimeContractDigest")
        }));
    }

    #[test]
    fn requiredness_drift_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join("contracts/docs-serving.schema.json");
        let mut schema: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        schema["$defs"]["DocsRequest"]["required"]
            .as_array_mut()
            .unwrap()
            .push(Value::String("accept".to_owned()));
        fs::write(&path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let findings = run(temp.path()).expect("parity run");
        assert!(
            findings
                .iter()
                .any(|item| item.detail.contains("model DocsRequest"))
        );
    }

    #[test]
    fn hierarchy_regression_stops_evaluation() {
        let temp = fixture_root();
        let path = temp.path().join("contracts/authority-topology.json");
        let mut topology: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        topology["prohibitedAuthorityEdges"] = json!([]);
        fs::write(&path, serde_json::to_vec_pretty(&topology).unwrap()).unwrap();
        let findings = run(temp.path()).expect("parity run");
        assert!(
            findings
                .iter()
                .any(|item| item.kind == "authority-topology")
        );
    }
}
