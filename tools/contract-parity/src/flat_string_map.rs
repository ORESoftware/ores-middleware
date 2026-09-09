//! Narrow compatibility for the independently authored docs header map.
//! This does not replace compiler-backed TJSV or normalize composed schemas.

use crate::Result;
use regex::Regex;
use serde_json::{Map, Value, json};

pub(super) fn is_anonymous(type_name: &str) -> Result<bool> {
    Ok(Regex::new(r"^\{\s*\.\.\.Record\s*<\s*string\s*>\s*;\s*\}$")?.is_match(type_name))
}

pub(super) fn normalize_unevaluated(object: &Map<String, Value>) -> Result<Option<Value>> {
    if !object.contains_key("unevaluatedProperties") {
        return Ok(None);
    }
    // Exactly this flat object: no named keys, refs, composition, conditions,
    // adjacent additionalProperties, or ignored constraints. Any extension
    // needs explicit semantic support rather than an optimistic rewrite.
    let expected = json!({
        "type": "object",
        "properties": {},
        "unevaluatedProperties": {"type": "string"}
    });
    if Some(object) != expected.as_object() {
        return Err("unsupported unevaluated dictionary shape in preflight".into());
    }
    Ok(Some(json!({
        "type": "object",
        "additionalProperties": {"type": "string"}
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{normalize_json_shape, normalize_tsp_type, parse_tsp_model};

    #[test]
    fn anonymous_and_named_string_maps_have_the_same_preflight_shape() {
        let expected = normalize_tsp_type("Record<string>", None).unwrap();
        for spelling in ["{ ...Record<string>; }", "{...Record < string > ;}"] {
            assert_eq!(normalize_tsp_type(spelling, None).unwrap(), expected);
            let source = format!("model Example {{\n  headers: {spelling};\n}}\n");
            let parsed = parse_tsp_model(&source, "Example").unwrap();
            assert_eq!(parsed["headers"]["shape"], expected);
            assert_eq!(parsed["headers"]["required"], true);
        }
    }

    #[test]
    fn anonymous_map_requiredness_and_decorators_remain_strict() {
        let source = "model Example {\n  headers?: { ...Record<string>; };\n}\n";
        let parsed = parse_tsp_model(source, "Example").unwrap();
        assert_eq!(parsed["headers"]["required"], false);
        assert!(normalize_tsp_type("{ ...Record<string>; }", Some(".*".into())).is_err());
    }

    #[test]
    fn unsupported_anonymous_or_injected_syntax_is_rejected() {
        for spelling in [
            "{ ...Record<int32>; }",
            "{ ...Record<string> }",
            "{ ...Record<string>; extra: string; }",
            "{ ...Record<string>; ...Other; }",
            "{ ...Record<Record<string>>; }",
            "{ [key: string]: string; }",
            "Record<string>; extra: string",
            "{ ...Record<string>; }; extra: string",
            "{ ...Record<string>; } | null",
        ] {
            let source = format!("model Example {{\n  headers: {spelling};\n}}\n");
            assert!(parse_tsp_model(&source, "Example").is_err(), "{spelling}");
        }
    }

    #[test]
    fn flat_json_dictionary_spellings_have_the_same_preflight_shape() {
        let old = json!({"type": "object", "additionalProperties": {"type": "string"}});
        let new = json!({
            "type": "object", "properties": {}, "unevaluatedProperties": {"type": "string"}
        });
        assert_eq!(
            normalize_json_shape(&old).unwrap(),
            normalize_json_shape(&new).unwrap()
        );
    }

    #[test]
    fn unevaluated_dictionary_extensions_are_not_silently_dropped() {
        let baseline = json!({
            "type": "object", "properties": {}, "unevaluatedProperties": {"type": "string"}
        });
        for (key, value) in [
            ("properties", json!({"exception": {"type": "integer"}})),
            ("patternProperties", json!({"^x": {"type": "integer"}})),
            ("allOf", json!([{"type": "object"}])),
            ("if", json!({"required": ["condition"]})),
            ("additionalProperties", json!(true)),
            ("minProperties", json!(1)),
            ("unevaluatedProperties", json!(true)),
            ("unevaluatedProperties", json!(false)),
            ("unevaluatedProperties", json!({"type": "integer"})),
            (
                "unevaluatedProperties",
                json!({"type": "string", "minLength": 1}),
            ),
        ] {
            let mut altered = baseline.clone();
            altered[key] = value;
            assert!(normalize_json_shape(&altered).is_err(), "{key}: {altered}");
        }
        let mut missing = baseline;
        missing.as_object_mut().unwrap().remove("properties");
        assert!(normalize_json_shape(&missing).is_err());
    }
}
