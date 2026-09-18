use ores_middleware::{
    RouteRateLimitBindingRequest, RouteRateLimitBindingResolutionError,
    RouteRateLimitBindingTable, RouteRateLimitBindingSource,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    runtime_valid: bool,
    config: RouteRateLimitBindingTable,
    request: Option<Request>,
    expect: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Request {
    method: String,
    path: String,
    route_template: Option<String>,
    operation_id: Option<String>,
}

#[test]
fn shared_route_binding_corpus_matches_rust_runtime() {
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../../contracts/route-rate-limit/fixtures/conformance.json"
    ))
    .expect("parse route binding conformance corpus");

    assert_eq!(
        corpus.schema,
        "ores.middleware.route-rate-limit-conformance/v1"
    );
    assert_eq!(corpus.cases.len(), 15);

    for case in corpus.cases {
        let issues = case.config.validate();
        assert_eq!(
            issues.is_empty(),
            case.runtime_valid,
            "{} runtime validation drift: {issues:?}",
            case.id
        );
        if !case.runtime_valid {
            continue;
        }

        let (Some(request), Some(expect)) = (case.request.as_ref(), case.expect.as_ref()) else {
            continue;
        };
        let request = RouteRateLimitBindingRequest {
            method: request.method.as_str(),
            path: request.path.as_str(),
            route_template: request.route_template.as_deref(),
            operation_id: request.operation_id.as_deref(),
        };
        let kind = expect
            .get("kind")
            .and_then(Value::as_str)
            .expect("expect.kind");

        if kind == "ambiguous" {
            let error = case.config.resolve(&request).expect_err("expected ambiguity");
            match error {
                RouteRateLimitBindingResolutionError::Ambiguous {
                    route_class_ids,
                    policy_ids,
                    ..
                } => {
                    assert_eq!(
                        route_class_ids,
                        string_array(expect.get("route_class_ids").expect("route_class_ids")),
                        "{} route-class ambiguity drift",
                        case.id
                    );
                    assert_eq!(
                        policy_ids,
                        string_array(expect.get("policy_ids").expect("policy_ids")),
                        "{} policy ambiguity drift",
                        case.id
                    );
                }
                other => panic!("{} expected ambiguity, got {other:?}", case.id),
            }
            continue;
        }

        let resolved = case.config.resolve(&request).expect("resolve binding");
        if kind == "none" {
            assert!(resolved.is_none(), "{} expected no binding", case.id);
            continue;
        }

        let resolved = resolved.expect("expected binding");
        let expected_policy_id = expect
            .get("policy_id")
            .and_then(Value::as_str)
            .expect("expect.policy_id");
        assert_eq!(resolved.policy_id, expected_policy_id, "{} policy drift", case.id);

        match kind {
            "route" => {
                assert_eq!(resolved.source, RouteRateLimitBindingSource::Route);
                assert_eq!(
                    resolved.route_class_id,
                    expect.get("route_class_id").and_then(Value::as_str),
                    "{} route class drift",
                    case.id
                );
            }
            "default" => {
                assert_eq!(resolved.source, RouteRateLimitBindingSource::Default);
                assert_eq!(resolved.route_class_id, None);
            }
            other => panic!("{} unsupported expectation kind {other}", case.id),
        }
    }
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("string array")
        .iter()
        .map(|entry| entry.as_str().expect("string entry").to_owned())
        .collect()
}
