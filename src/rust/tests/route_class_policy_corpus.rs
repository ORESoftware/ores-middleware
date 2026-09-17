use std::fs;
use std::path::PathBuf;

use ores_middleware::{
    RouteClassPolicyResolutionError, RouteClassPolicyTable, resolve_route_class_policy_for_test_only,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    table: RouteClassPolicyTable,
    route_class_id: Option<String>,
    expect: Value,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/route-class-policy/fixtures/conformance.json")
}

#[test]
fn shared_route_class_policy_corpus() {
    let corpus: Corpus =
        serde_json::from_slice(&fs::read(fixture_path()).expect("read route class policy corpus"))
            .expect("decode route class policy corpus");

    assert_eq!(corpus.cases.len(), 15, "reviewed corpus size drift");

    for case in corpus.cases {
        let expected_valid = case
            .expect
            .get("valid")
            .and_then(Value::as_bool)
            .expect("expect.valid");
        let violations = case.table.validate();

        if !expected_valid {
            assert!(
                !violations.is_empty(),
                "{} should fail validation",
                case.id
            );
            for code in case
                .expect
                .get("codes")
                .and_then(Value::as_array)
                .expect("expect.codes")
            {
                let code = code.as_str().expect("string violation code");
                assert!(
                    violations.iter().any(|issue| issue.code == code),
                    "{} missing expected violation {code}; got {:?}",
                    case.id,
                    violations
                );
            }
            continue;
        }

        assert!(
            violations.is_empty(),
            "{} unexpected validation issues: {:?}",
            case.id,
            violations
        );

        let result = resolve_route_class_policy_for_test_only(
            &case.table,
            case.route_class_id.as_deref(),
        );

        if case.expect.get("resolution_error").is_some() {
            assert!(
                matches!(
                    result,
                    Err(RouteClassPolicyResolutionError::UnknownRouteClass { .. })
                ),
                "{} expected unknown route class resolution failure, got {:?}",
                case.id,
                result
            );
            continue;
        }

        let resolved = result.expect("valid route class policy resolution");
        let expected = case.expect.get("resolved").expect("expect.resolved");
        assert_eq!(
            resolved.route_class_id,
            expected
                .get("route_class_id")
                .and_then(Value::as_str)
                .expect("resolved route_class_id"),
            "{} route class drift",
            case.id
        );

        let expected_lineage = expected
            .get("lineage")
            .and_then(Value::as_array)
            .expect("resolved lineage")
            .iter()
            .map(|value| value.as_str().expect("lineage string"))
            .collect::<Vec<_>>();
        assert_eq!(
            resolved.lineage.iter().map(String::as_str).collect::<Vec<_>>(),
            expected_lineage,
            "{} lineage drift",
            case.id
        );

        let policies = serde_json::to_value(&resolved.policies).expect("serialize resolved policies");
        for (key, expected_value) in expected
            .get("policies")
            .and_then(Value::as_object)
            .expect("resolved policies")
        {
            assert_eq!(
                policies.get(key),
                Some(expected_value),
                "{} policy {key} drift",
                case.id
            );
        }
    }
}
