use ores_middleware::{
    RouteRateLimitBindingRequest, RouteRateLimitBindingResolutionError,
    RouteRateLimitBindingSource, RouteRateLimitBindingTable,
};
use serde::Deserialize;
use serde_json::Value;

const REQUEST_CORPUS: &str =
    include_str!("../../../contracts/route-rate-limit/fixtures/request-hardening.json");

#[derive(Debug, Deserialize)]
struct Corpus {
    base_config: Value,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    #[serde(rename = "schema_valid")]
    _schema_valid: bool,
    runtime_valid: bool,
    request: Value,
    expect: Option<Value>,
}

fn request_from_value(value: &Value) -> RouteRateLimitBindingRequest<'_> {
    RouteRateLimitBindingRequest {
        method: value
            .get("method")
            .and_then(Value::as_str)
            .expect("request.method"),
        path: value
            .get("path")
            .and_then(Value::as_str)
            .expect("request.path"),
        route_template: value.get("route_template").and_then(Value::as_str),
        operation_id: value.get("operation_id").and_then(Value::as_str),
    }
}

#[test]
fn request_hardening_corpus_matches_runtime_admission_and_resolution() {
    let corpus: Corpus = serde_json::from_str(REQUEST_CORPUS).expect("request hardening corpus");
    assert_eq!(corpus.cases.len(), 15);

    let table: RouteRateLimitBindingTable =
        serde_json::from_value(corpus.base_config.clone()).expect("base binding table");

    for case in &corpus.cases {
        let request = request_from_value(&case.request);
        let issues = request.validate();
        assert_eq!(
            issues.is_empty(),
            case.runtime_valid,
            "{} request validation drift: {issues:?}",
            case.id
        );

        if !case.runtime_valid {
            match table.resolve(&request) {
                Err(RouteRateLimitBindingResolutionError::InvalidRequest { violations }) => {
                    assert!(!violations.is_empty(), "{} missing violations", case.id);
                }
                other => panic!("{} expected invalid request, got {other:?}", case.id),
            }
            continue;
        }

        let expected = case.expect.as_ref().expect("valid case expectation");
        let kind = expected
            .get("kind")
            .and_then(Value::as_str)
            .expect("expect.kind");
        let resolved = table.resolve(&request).expect("valid resolution");

        match kind {
            "route" => {
                let resolved = resolved.expect("route binding");
                assert_eq!(resolved.source, RouteRateLimitBindingSource::Route);
                assert_eq!(
                    resolved.route_class_id,
                    expected.get("route_class_id").and_then(Value::as_str),
                    "{} route class drift",
                    case.id
                );
                assert_eq!(
                    resolved.policy_id,
                    expected
                        .get("policy_id")
                        .and_then(Value::as_str)
                        .expect("expect.policy_id"),
                    "{} policy drift",
                    case.id
                );
            }
            "default" => {
                let resolved = resolved.expect("default binding");
                assert_eq!(resolved.source, RouteRateLimitBindingSource::Default);
                assert_eq!(resolved.route_class_id, None);
                assert_eq!(
                    resolved.policy_id,
                    expected
                        .get("policy_id")
                        .and_then(Value::as_str)
                        .expect("expect.policy_id"),
                    "{} default policy drift",
                    case.id
                );
            }
            "none" => assert!(resolved.is_none(), "{} expected no binding", case.id),
            other => panic!("{} unsupported expectation kind {other}", case.id),
        }
    }
}

#[test]
fn invalid_table_cannot_resolve_when_callers_skip_validation() {
    let mut table: RouteRateLimitBindingTable = serde_json::from_value(
        serde_json::from_str::<Corpus>(REQUEST_CORPUS)
            .expect("request hardening corpus")
            .base_config,
    )
    .expect("base binding table");
    table.schema = "route-bindings/v0".into();

    let request = RouteRateLimitBindingRequest {
        method: "GET",
        path: "/users/42",
        route_template: Some("/users/{id}"),
        operation_id: Some("users.read"),
    };

    match table.resolve(&request) {
        Err(RouteRateLimitBindingResolutionError::InvalidTable { violations }) => {
            assert!(
                violations
                    .iter()
                    .any(|issue| issue.code == "invalid-binding-schema")
            );
        }
        other => panic!("expected invalid table, got {other:?}"),
    }
}
