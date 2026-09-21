use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const ROUTE_RATE_LIMIT_BINDING_SCHEMA: &str = "ores.middleware.route-rate-limit-bindings/v1";
pub const MAX_ROUTE_RATE_LIMIT_BINDINGS: usize = 128;
pub const MAX_ROUTE_METHODS: usize = 8;
pub const MAX_ROUTE_RATE_LIMIT_REQUEST_PATH_LENGTH: usize = 4096;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRateLimitBindingSelector {
    #[serde(default)]
    pub methods: Vec<String>,
    pub path_template: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRateLimitBinding {
    pub route_class_id: String,
    pub policy_id: String,
    pub selector: RouteRateLimitBindingSelector,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRateLimitBindingTable {
    pub schema: String,
    pub default_policy_id: Option<String>,
    #[serde(default)]
    pub routes: Vec<RouteRateLimitBinding>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RouteRateLimitBindingRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub route_template: Option<&'a str>,
    pub operation_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RouteRateLimitBindingSource {
    Route,
    Default,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ResolvedRouteRateLimitBinding<'a> {
    pub route_class_id: Option<&'a str>,
    pub policy_id: &'a str,
    pub source: RouteRateLimitBindingSource,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteRateLimitBindingViolation {
    pub code: &'static str,
    pub path: String,
    pub message: &'static str,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RouteRateLimitBindingResolutionError {
    InvalidTable {
        violations: Vec<RouteRateLimitBindingViolation>,
    },
    InvalidRequest {
        violations: Vec<RouteRateLimitBindingViolation>,
    },
    Ambiguous {
        method: String,
        path: String,
        operation_id: Option<String>,
        route_class_ids: Vec<String>,
        policy_ids: Vec<String>,
    },
}

impl Default for RouteRateLimitBindingTable {
    fn default() -> Self {
        Self {
            schema: ROUTE_RATE_LIMIT_BINDING_SCHEMA.to_owned(),
            default_policy_id: None,
            routes: Vec::new(),
        }
    }
}

impl RouteRateLimitBindingRequest<'_> {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteRateLimitBindingViolation> {
        let mut violations = Vec::new();

        if !valid_request_http_method(self.method) {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-request-method",
                path: "method".into(),
                message: "request method must be a bounded ASCII HTTP token",
            });
        }

        let path_len = self.path.chars().count();
        let basic_path_valid = path_len > 0
            && path_len <= MAX_ROUTE_RATE_LIMIT_REQUEST_PATH_LENGTH
            && self.path.starts_with('/')
            && !self
                .path
                .chars()
                .any(|ch| matches!(ch, '#' | '\r' | '\n' | '\0'));

        if !basic_path_valid {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-request-path",
                path: "path".into(),
                message: "request path must be bounded, absolute, and free of fragments/control separators",
            });
        } else {
            let request_path = self
                .path
                .split_once('?')
                .map_or(self.path, |(path, _)| path);
            if request_path.contains("//") {
                violations.push(RouteRateLimitBindingViolation {
                    code: "repeated-request-path-separator",
                    path: "path".into(),
                    message: "request path must not contain repeated slash separators",
                });
            }
            if path_segments(request_path)
                .into_iter()
                .any(|segment| segment == "." || segment == "..")
            {
                violations.push(RouteRateLimitBindingViolation {
                    code: "dot-request-path-segment",
                    path: "path".into(),
                    message: "request path must not contain literal dot segments",
                });
            }
        }

        if let Some(route_template) = self.route_template {
            if validate_path_template(route_template).is_err() {
                violations.push(RouteRateLimitBindingViolation {
                    code: "invalid-request-route-template",
                    path: "route_template".into(),
                    message: "request route_template must use the canonical configured-template grammar",
                });
            } else if basic_path_valid && !path_template_matches(route_template, self.path) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "route-template-path-mismatch",
                    path: "route_template".into(),
                    message: "trusted route_template must describe the concrete request path",
                });
            }
        }

        if self
            .operation_id
            .is_some_and(|value| !valid_operation_id(value))
        {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-request-operation-id",
                path: "operation_id".into(),
                message: "request operation_id must be a bounded stable ASCII identifier",
            });
        }

        violations
    }
}

impl RouteRateLimitBindingSelector {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteRateLimitBindingViolation> {
        let mut violations = Vec::new();

        if self.methods.is_empty() && self.path_template.is_none() && self.operation_id.is_none() {
            violations.push(RouteRateLimitBindingViolation {
                code: "empty-route-selector",
                path: "selector".into(),
                message: "route selector must constrain method, path_template, or operation_id",
            });
        }

        if self.methods.len() > MAX_ROUTE_METHODS {
            violations.push(RouteRateLimitBindingViolation {
                code: "too-many-http-methods",
                path: "selector.methods".into(),
                message: "route selector exceeds the bounded HTTP method count",
            });
        }

        let mut seen_methods = BTreeSet::new();
        for (index, method) in self.methods.iter().enumerate() {
            if !valid_http_method(method) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "invalid-http-method",
                    path: format!("selector.methods[{index}]"),
                    message: "HTTP methods must be bounded canonical uppercase ASCII tokens",
                });
            }
            if !seen_methods.insert(method) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "duplicate-http-method",
                    path: format!("selector.methods[{index}]"),
                    message: "HTTP method may appear only once in a selector",
                });
            }
        }

        if let Some(template) = self.path_template.as_deref()
            && let Err(code) = validate_path_template(template)
        {
            violations.push(RouteRateLimitBindingViolation {
                code,
                path: "selector.path_template".into(),
                message: "path_template must be a bounded normalized absolute route template",
            });
        }

        if self
            .operation_id
            .as_deref()
            .is_some_and(|value| !valid_operation_id(value))
        {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-operation-id",
                path: "selector.operation_id".into(),
                message: "operation_id must be a bounded stable ASCII identifier",
            });
        }

        violations
    }

    fn match_score(&self, request: &RouteRateLimitBindingRequest<'_>) -> Option<u32> {
        if !self.methods.is_empty()
            && !self
                .methods
                .iter()
                .any(|method| method.eq_ignore_ascii_case(request.method))
        {
            return None;
        }

        if let Some(operation_id) = self.operation_id.as_deref()
            && request.operation_id != Some(operation_id)
        {
            return None;
        }

        let mut score = u32::from(!self.methods.is_empty());
        if self.operation_id.is_some() {
            score += 10_000;
        }

        if let Some(template) = self.path_template.as_deref() {
            if !path_template_matches(template, request.path) {
                return None;
            }
            let registered_template_match = request.route_template == Some(template);
            score += 100;
            score += static_segment_count(template) * 10;
            if registered_template_match {
                score += 5;
            }
        }

        Some(score)
    }
}

impl RouteRateLimitBindingTable {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteRateLimitBindingViolation> {
        let mut violations = Vec::new();

        if self.schema != ROUTE_RATE_LIMIT_BINDING_SCHEMA {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-binding-schema",
                path: "schema".into(),
                message: "route binding schema identifier is not supported",
            });
        }

        if self.routes.len() > MAX_ROUTE_RATE_LIMIT_BINDINGS {
            violations.push(RouteRateLimitBindingViolation {
                code: "too-many-route-bindings",
                path: "routes".into(),
                message: "route binding table exceeds its bounded route count",
            });
        }

        if self
            .default_policy_id
            .as_deref()
            .is_some_and(|value| !valid_policy_id(value))
        {
            violations.push(RouteRateLimitBindingViolation {
                code: "invalid-policy-id",
                path: "default_policy_id".into(),
                message: "default_policy_id must be a bounded stable policy identifier",
            });
        }

        let mut route_classes = BTreeSet::new();
        let mut selectors = BTreeSet::new();
        for (index, route) in self.routes.iter().enumerate() {
            let prefix = format!("routes[{index}]");

            if !valid_route_class_id(&route.route_class_id) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "invalid-route-class-id",
                    path: format!("{prefix}.route_class_id"),
                    message: "route_class_id must be a bounded stable lowercase identifier",
                });
            }
            if !route_classes.insert(route.route_class_id.as_str()) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "duplicate-route-class-id",
                    path: format!("{prefix}.route_class_id"),
                    message: "route_class_id must be unique within a binding table",
                });
            }

            if !valid_policy_id(&route.policy_id) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "invalid-policy-id",
                    path: format!("{prefix}.policy_id"),
                    message: "policy_id must be a bounded stable policy identifier",
                });
            }

            violations.extend(route.selector.validate().into_iter().map(|mut violation| {
                violation.path = format!("{prefix}.{}", violation.path);
                violation
            }));

            if !selectors.insert(canonical_selector_key(&route.selector)) {
                violations.push(RouteRateLimitBindingViolation {
                    code: "duplicate-route-selector",
                    path: format!("{prefix}.selector"),
                    message: "identical route selectors are ambiguous and must be combined",
                });
            }
        }

        violations
    }

    pub fn resolve<'a>(
        &'a self,
        request: &RouteRateLimitBindingRequest<'_>,
    ) -> Result<Option<ResolvedRouteRateLimitBinding<'a>>, RouteRateLimitBindingResolutionError>
    {
        let table_violations = self.validate();
        if !table_violations.is_empty() {
            return Err(RouteRateLimitBindingResolutionError::InvalidTable {
                violations: table_violations,
            });
        }
        let request_violations = request.validate();
        if !request_violations.is_empty() {
            return Err(RouteRateLimitBindingResolutionError::InvalidRequest {
                violations: request_violations,
            });
        }

        let mut best_score = None;
        let mut matches: Vec<&RouteRateLimitBinding> = Vec::new();

        for route in &self.routes {
            let Some(score) = route.selector.match_score(request) else {
                continue;
            };
            match best_score {
                None => {
                    best_score = Some(score);
                    matches.push(route);
                }
                Some(current) if score > current => {
                    best_score = Some(score);
                    matches.clear();
                    matches.push(route);
                }
                Some(current) if score == current => matches.push(route),
                Some(_) => {}
            }
        }

        match matches.as_slice() {
            [] => Ok(self.default_policy_id.as_deref().map(|policy_id| {
                ResolvedRouteRateLimitBinding {
                    route_class_id: None,
                    policy_id,
                    source: RouteRateLimitBindingSource::Default,
                }
            })),
            [route] => Ok(Some(ResolvedRouteRateLimitBinding {
                route_class_id: Some(route.route_class_id.as_str()),
                policy_id: route.policy_id.as_str(),
                source: RouteRateLimitBindingSource::Route,
            })),
            many => {
                let mut route_class_ids = many
                    .iter()
                    .map(|route| route.route_class_id.clone())
                    .collect::<Vec<_>>();
                route_class_ids.sort();
                route_class_ids.dedup();
                let mut policy_ids = many
                    .iter()
                    .map(|route| route.policy_id.clone())
                    .collect::<Vec<_>>();
                policy_ids.sort();
                policy_ids.dedup();
                Err(RouteRateLimitBindingResolutionError::Ambiguous {
                    method: request.method.to_owned(),
                    path: request.path.to_owned(),
                    operation_id: request.operation_id.map(str::to_owned),
                    route_class_ids,
                    policy_ids,
                })
            }
        }
    }
}

fn canonical_selector_key(
    selector: &RouteRateLimitBindingSelector,
) -> (Vec<String>, Option<String>, Option<String>) {
    let mut methods = selector.methods.clone();
    methods.sort();
    (
        methods,
        selector.path_template.clone(),
        selector.operation_id.clone(),
    )
}

fn valid_route_class_id(value: &str) -> bool {
    stable_lower_id(value, 80, false)
}

fn valid_policy_id(value: &str) -> bool {
    stable_lower_id(value, 160, true)
}

fn stable_lower_id(value: &str, max_len: usize, allow_colon: bool) -> bool {
    if value.is_empty() || value.len() > max_len {
        return false;
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut previous_separator = false;
    for byte in bytes {
        let separator = matches!(*byte, b'-' | b'_' | b'.') || (allow_colon && *byte == b':');
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() {
            previous_separator = false;
        } else if separator && !previous_separator {
            previous_separator = true;
        } else {
            return false;
        }
    }
    !previous_separator
}

fn valid_request_http_method(method: &str) -> bool {
    let bytes = method.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

fn valid_http_method(method: &str) -> bool {
    let bytes = method.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_operation_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn valid_parameter_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn validate_path_template(template: &str) -> Result<(), &'static str> {
    if template.is_empty() || template.len() > 512 || !template.starts_with('/') {
        return Err("invalid-path-template");
    }
    if template.contains('?') || template.contains('#') {
        return Err("invalid-path-template");
    }
    if template.len() > 1 && template.ends_with('/') {
        return Err("non-canonical-path-template");
    }

    let segments = path_segments(template);
    for (index, segment) in segments.iter().enumerate() {
        if segment.is_empty() {
            return Err("empty-path-segment");
        }
        if *segment == "." || *segment == ".." {
            return Err("dot-path-segment");
        }
        if *segment == "*" {
            if index + 1 != segments.len() {
                return Err("catch-all-must-be-terminal");
            }
            continue;
        }
        if let Some(name) = segment.strip_prefix('{') {
            let Some(name) = name.strip_suffix('}') else {
                return Err("invalid-path-parameter");
            };
            if !valid_parameter_name(name) {
                return Err("invalid-path-parameter");
            }
            continue;
        }
        if let Some(name) = segment.strip_prefix(':') {
            if !valid_parameter_name(name) {
                return Err("invalid-path-parameter");
            }
            continue;
        }
        if segment.contains('{') || segment.contains('}') || segment.contains('*') {
            return Err("invalid-path-segment");
        }
    }
    Ok(())
}

fn path_template_matches(template: &str, request_path: &str) -> bool {
    let request_path = request_path
        .split_once('?')
        .map_or(request_path, |(path, _)| path);
    let template_segments = path_segments(template);
    let request_segments = path_segments(request_path);

    let mut path_index = 0;
    for (template_index, template_segment) in template_segments.iter().enumerate() {
        if *template_segment == "*" && template_index + 1 == template_segments.len() {
            return true;
        }
        let Some(path_segment) = request_segments.get(path_index) else {
            return false;
        };
        if is_parameter_segment(template_segment) {
            if path_segment.is_empty() {
                return false;
            }
        } else if template_segment != path_segment {
            return false;
        }
        path_index += 1;
    }

    path_index == request_segments.len()
}

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        Vec::new()
    } else {
        path.strip_prefix('/').unwrap_or(path).split('/').collect()
    }
}

fn is_parameter_segment(segment: &str) -> bool {
    (segment.starts_with('{') && segment.ends_with('}') && segment.len() > 2)
        || (segment.starts_with(':') && segment.len() > 1)
}

fn static_segment_count(template: &str) -> u32 {
    path_segments(template)
        .into_iter()
        .filter(|segment| !is_parameter_segment(segment) && *segment != "*")
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(
        route_class_id: &str,
        policy_id: &str,
        methods: &[&str],
        path_template: Option<&str>,
        operation_id: Option<&str>,
    ) -> RouteRateLimitBinding {
        RouteRateLimitBinding {
            route_class_id: route_class_id.into(),
            policy_id: policy_id.into(),
            selector: RouteRateLimitBindingSelector {
                methods: methods.iter().map(|value| (*value).into()).collect(),
                path_template: path_template.map(str::to_owned),
                operation_id: operation_id.map(str::to_owned),
            },
        }
    }

    #[test]
    fn resolves_policy_id_without_owning_numeric_policy() {
        let table = RouteRateLimitBindingTable {
            schema: ROUTE_RATE_LIMIT_BINDING_SCHEMA.into(),
            default_policy_id: Some("public-read-default".into()),
            routes: vec![binding(
                "auth-login",
                "auth:login-strict",
                &["POST"],
                Some("/auth/login"),
                Some("auth.login"),
            )],
        };
        assert!(table.validate().is_empty());
        let resolved = table
            .resolve(&RouteRateLimitBindingRequest {
                method: "POST",
                path: "/auth/login",
                route_template: Some("/auth/login"),
                operation_id: Some("auth.login"),
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.route_class_id, Some("auth-login"));
        assert_eq!(resolved.policy_id, "auth:login-strict");
        assert_eq!(resolved.source, RouteRateLimitBindingSource::Route);
    }

    #[test]
    fn operation_id_beats_parameterized_path() {
        let table = RouteRateLimitBindingTable {
            routes: vec![
                binding(
                    "job-path",
                    "jobs:default",
                    &["POST"],
                    Some("/jobs/{job_id}"),
                    None,
                ),
                binding(
                    "job-retry",
                    "jobs:retry",
                    &["POST"],
                    None,
                    Some("jobs.retry"),
                ),
            ],
            ..Default::default()
        };
        let resolved = table
            .resolve(&RouteRateLimitBindingRequest {
                method: "POST",
                path: "/jobs/42",
                route_template: Some("/jobs/{job_id}"),
                operation_id: Some("jobs.retry"),
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.route_class_id, Some("job-retry"));
        assert_eq!(resolved.policy_id, "jobs:retry");
    }

    #[test]
    fn parameterized_route_matches_concrete_path_and_query() {
        let table = RouteRateLimitBindingTable {
            routes: vec![binding(
                "ledger-entry",
                "ledger:read",
                &["GET"],
                Some("/ledger/{ledger_id}/entries/:entry_id"),
                None,
            )],
            ..Default::default()
        };
        assert!(table.validate().is_empty());
        assert_eq!(
            table
                .resolve(&RouteRateLimitBindingRequest {
                    method: "GET",
                    path: "/ledger/a/entries/42?expand=true",
                    route_template: None,
                    operation_id: None,
                })
                .unwrap()
                .unwrap()
                .policy_id,
            "ledger:read"
        );
    }

    #[test]
    fn default_policy_is_explicit_fallback() {
        let table = RouteRateLimitBindingTable {
            default_policy_id: Some("public:default".into()),
            ..Default::default()
        };
        let resolved = table
            .resolve(&RouteRateLimitBindingRequest {
                method: "GET",
                path: "/unmatched",
                route_template: None,
                operation_id: None,
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.route_class_id, None);
        assert_eq!(resolved.policy_id, "public:default");
        assert_eq!(resolved.source, RouteRateLimitBindingSource::Default);
    }

    #[test]
    fn no_match_without_default_stays_unbound() {
        let table = RouteRateLimitBindingTable::default();
        assert!(
            table
                .resolve(&RouteRateLimitBindingRequest {
                    method: "GET",
                    path: "/unmatched",
                    route_template: None,
                    operation_id: None,
                })
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn equal_specificity_fails_closed_with_stable_evidence() {
        let table = RouteRateLimitBindingTable {
            routes: vec![
                binding("users-b", "users:beta", &["GET"], Some("/users/:id"), None),
                binding(
                    "users-a",
                    "users:alpha",
                    &["GET"],
                    Some("/users/{id}"),
                    None,
                ),
            ],
            ..Default::default()
        };
        let error = table
            .resolve(&RouteRateLimitBindingRequest {
                method: "GET",
                path: "/users/42",
                route_template: None,
                operation_id: None,
            })
            .unwrap_err();
        assert_eq!(
            error,
            RouteRateLimitBindingResolutionError::Ambiguous {
                method: "GET".into(),
                path: "/users/42".into(),
                operation_id: None,
                route_class_ids: vec!["users-a".into(), "users-b".into()],
                policy_ids: vec!["users:alpha".into(), "users:beta".into()],
            }
        );
    }

    #[test]
    fn rejects_duplicate_route_class_and_selector() {
        let route = binding("search", "search:read", &["GET"], Some("/search"), None);
        let table = RouteRateLimitBindingTable {
            routes: vec![
                route.clone(),
                RouteRateLimitBinding {
                    policy_id: "search:other".into(),
                    ..route
                },
            ],
            ..Default::default()
        };
        let violations = table.validate();
        assert!(
            violations
                .iter()
                .any(|issue| issue.code == "duplicate-route-class-id")
        );
        assert!(
            violations
                .iter()
                .any(|issue| issue.code == "duplicate-route-selector")
        );
    }

    #[test]
    fn rejects_invalid_identifiers_and_method_tokens() {
        let table = RouteRateLimitBindingTable {
            default_policy_id: Some("Bad Policy".into()),
            routes: vec![binding("BadRoute", "also bad", &["-"], Some("/ok"), None)],
            ..Default::default()
        };
        let violations = table.validate();
        assert!(
            violations
                .iter()
                .any(|issue| issue.code == "invalid-route-class-id")
        );
        assert!(
            violations
                .iter()
                .filter(|issue| issue.code == "invalid-policy-id")
                .count()
                >= 2
        );
        assert!(
            violations
                .iter()
                .any(|issue| issue.code == "invalid-http-method")
        );
    }

    #[test]
    fn rejects_noncanonical_path_parameters_and_dot_segments() {
        for template in ["/users/{bad-name}", "/users/{id", "/users/..", "/a/*/b"] {
            let table = RouteRateLimitBindingTable {
                routes: vec![binding(
                    "bad-route",
                    "bad:route",
                    &["GET"],
                    Some(template),
                    None,
                )],
                ..Default::default()
            };
            assert!(
                table
                    .validate()
                    .iter()
                    .any(|issue| issue.path.ends_with("selector.path_template"))
            );
        }
    }

    #[test]
    fn table_and_method_counts_are_bounded() {
        let route = binding("route", "route:policy", &["GET"], Some("/route"), None);
        let mut table = RouteRateLimitBindingTable {
            routes: vec![route; MAX_ROUTE_RATE_LIMIT_BINDINGS + 1],
            ..Default::default()
        };
        assert!(
            table
                .validate()
                .iter()
                .any(|issue| issue.code == "too-many-route-bindings")
        );
        table.routes.truncate(1);
        table.routes[0].selector.methods = vec!["GET".into(); MAX_ROUTE_METHODS + 1];
        assert!(
            table
                .validate()
                .iter()
                .any(|issue| issue.code == "too-many-http-methods")
        );
    }
}
