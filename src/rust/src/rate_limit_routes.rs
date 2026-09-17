use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::rate_limit_v2::{RateLimitPolicyV2, RateLimitPolicyViolation};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RateLimitRouteSelector {
    #[serde(default)]
    pub methods: Vec<String>,
    pub path_template: Option<String>,
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRateLimitRule {
    pub selector: RateLimitRouteSelector,
    pub policy: RateLimitPolicyV2,
}

#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteRateLimitTable {
    pub default_policy: Option<RateLimitPolicyV2>,
    #[serde(default)]
    pub routes: Vec<RouteRateLimitRule>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RouteRateLimitRequest<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub route_template: Option<&'a str>,
    pub operation_id: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RouteRateLimitPolicySource {
    Route,
    Default,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedRouteRateLimitPolicy<'a> {
    pub policy: &'a RateLimitPolicyV2,
    pub source: RouteRateLimitPolicySource,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteRateLimitViolation {
    pub code: &'static str,
    pub path: String,
    pub message: &'static str,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RouteRateLimitResolutionError {
    Ambiguous {
        method: String,
        path: String,
        operation_id: Option<String>,
        policy_ids: Vec<String>,
    },
}

impl RateLimitRouteSelector {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteRateLimitViolation> {
        let mut violations = Vec::new();

        if self.methods.is_empty() && self.path_template.is_none() && self.operation_id.is_none() {
            violations.push(RouteRateLimitViolation {
                code: "empty-route-selector",
                path: "selector".into(),
                message: "route selector must constrain method, path_template, or operation_id",
            });
        }

        let mut seen_methods = BTreeSet::new();
        for (index, method) in self.methods.iter().enumerate() {
            if method.is_empty()
                || !method
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
            {
                violations.push(RouteRateLimitViolation {
                    code: "invalid-http-method",
                    path: format!("selector.methods[{index}]"),
                    message: "HTTP methods must be canonical uppercase ASCII tokens",
                });
            }
            if !seen_methods.insert(method) {
                violations.push(RouteRateLimitViolation {
                    code: "duplicate-http-method",
                    path: format!("selector.methods[{index}]"),
                    message: "HTTP method may appear only once in a selector",
                });
            }
        }

        if let Some(template) = &self.path_template
            && let Err(code) = validate_path_template(template)
        {
            violations.push(RouteRateLimitViolation {
                code,
                path: "selector.path_template".into(),
                message: "path_template must be an absolute normalized route template",
            });
        }

        if self.operation_id.as_deref().is_some_and(|value| {
            value.is_empty()
                || value.len() > 160
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
                })
        }) {
            violations.push(RouteRateLimitViolation {
                code: "invalid-operation-id",
                path: "selector.operation_id".into(),
                message: "operation_id must be a bounded stable ASCII identifier",
            });
        }

        violations
    }

    fn match_score(&self, request: &RouteRateLimitRequest<'_>) -> Option<u32> {
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
            let registered_template_match = request.route_template == Some(template);
            if !registered_template_match && !path_template_matches(template, request.path) {
                return None;
            }
            score += 100;
            score += static_segment_count(template) * 10;
            if registered_template_match {
                score += 5;
            }
        }

        Some(score)
    }
}

impl RouteRateLimitTable {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteRateLimitViolation> {
        let mut violations = Vec::new();
        let mut selectors = BTreeSet::new();

        if let Some(policy) = &self.default_policy {
            append_policy_violations(&mut violations, "default_policy", policy.validate());
        }

        for (index, rule) in self.routes.iter().enumerate() {
            let prefix = format!("routes[{index}]");
            violations.extend(rule.selector.validate().into_iter().map(|mut violation| {
                violation.path = format!("{prefix}.{}", violation.path);
                violation
            }));
            append_policy_violations(
                &mut violations,
                &format!("{prefix}.policy"),
                rule.policy.validate(),
            );

            if !selectors.insert(canonical_selector_key(&rule.selector)) {
                violations.push(RouteRateLimitViolation {
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
        request: &RouteRateLimitRequest<'_>,
    ) -> Result<Option<ResolvedRouteRateLimitPolicy<'a>>, RouteRateLimitResolutionError> {
        let mut best_score = None;
        let mut matches: Vec<&RouteRateLimitRule> = Vec::new();

        for rule in &self.routes {
            let Some(score) = rule.selector.match_score(request) else {
                continue;
            };
            match best_score {
                None => {
                    best_score = Some(score);
                    matches.push(rule);
                }
                Some(current) if score > current => {
                    best_score = Some(score);
                    matches.clear();
                    matches.push(rule);
                }
                Some(current) if score == current => matches.push(rule),
                Some(_) => {}
            }
        }

        match matches.as_slice() {
            [] => Ok(self
                .default_policy
                .as_ref()
                .map(|policy| ResolvedRouteRateLimitPolicy {
                    policy,
                    source: RouteRateLimitPolicySource::Default,
                })),
            [rule] => Ok(Some(ResolvedRouteRateLimitPolicy {
                policy: &rule.policy,
                source: RouteRateLimitPolicySource::Route,
            })),
            many => Err(RouteRateLimitResolutionError::Ambiguous {
                method: request.method.to_owned(),
                path: request.path.to_owned(),
                operation_id: request.operation_id.map(str::to_owned),
                policy_ids: many
                    .iter()
                    .map(|rule| rule.policy.policy_id.clone())
                    .collect(),
            }),
        }
    }
}

fn canonical_selector_key(
    selector: &RateLimitRouteSelector,
) -> (Vec<String>, Option<String>, Option<String>) {
    let mut methods = selector.methods.clone();
    methods.sort();
    (
        methods,
        selector.path_template.clone(),
        selector.operation_id.clone(),
    )
}

fn append_policy_violations(
    target: &mut Vec<RouteRateLimitViolation>,
    prefix: &str,
    source: Vec<RateLimitPolicyViolation>,
) {
    target.extend(source.into_iter().map(|violation| RouteRateLimitViolation {
        code: violation.code,
        path: format!("{prefix}.{}", violation.path),
        message: violation.message,
    }));
}

fn validate_path_template(template: &str) -> Result<(), &'static str> {
    if !template.starts_with('/') || template.contains('?') || template.contains('#') {
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
        if *segment == "*" && index + 1 != segments.len() {
            return Err("catch-all-must-be-terminal");
        }
        if segment.starts_with('{') && (!segment.ends_with('}') || segment.len() <= 2) {
            return Err("invalid-path-parameter");
        }
        if segment.starts_with(':') && segment.len() == 1 {
            return Err("invalid-path-parameter");
        }
    }
    Ok(())
}

fn path_template_matches(template: &str, request_path: &str) -> bool {
    let request_path = request_path
        .split_once('?')
        .map_or(request_path, |(path, _)| path);
    let template_segments = path_segments(template);
    let path_segments = path_segments(request_path);

    let mut path_index = 0;
    for (template_index, template_segment) in template_segments.iter().enumerate() {
        if *template_segment == "*" && template_index + 1 == template_segments.len() {
            return true;
        }
        let Some(path_segment) = path_segments.get(path_index) else {
            return false;
        };
        if !is_parameter_segment(template_segment) && template_segment != path_segment {
            return false;
        }
        path_index += 1;
    }

    path_index == path_segments.len()
}

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        Vec::new()
    } else {
        path.trim_matches('/').split('/').collect()
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
    use crate::{
        middleware_order::OperationClass,
        rate_limit_v2::{RateLimitEnforcementMode, RateLimitPolicyV2},
    };

    use super::*;

    fn policy(id: &str, operation_class: OperationClass) -> RateLimitPolicyV2 {
        let mut policy = RateLimitPolicyV2::audit_for(operation_class);
        policy.policy_id = id.into();
        policy.enforcement_mode = RateLimitEnforcementMode::Audit;
        policy
    }

    #[test]
    fn resolves_different_rates_for_different_routes() {
        let mut search = policy("search-read", OperationClass::PublicRead);
        search.capacity = 120;
        let mut login = policy("login-attempt", OperationClass::AuthAttempt);
        login.capacity = 8;

        let table = RouteRateLimitTable {
            default_policy: Some(policy("default-read", OperationClass::PublicRead)),
            routes: vec![
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["GET".into()],
                        path_template: Some("/search".into()),
                        operation_id: None,
                    },
                    policy: search,
                },
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["POST".into()],
                        path_template: Some("/auth/login".into()),
                        operation_id: Some("auth.login".into()),
                    },
                    policy: login,
                },
            ],
        };

        let resolved = table
            .resolve(&RouteRateLimitRequest {
                method: "POST",
                path: "/auth/login",
                route_template: Some("/auth/login"),
                operation_id: Some("auth.login"),
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.policy.policy_id, "login-attempt");
        assert_eq!(resolved.policy.capacity, 8);
    }

    #[test]
    fn parameterized_route_matches_concrete_path() {
        let table = RouteRateLimitTable {
            default_policy: None,
            routes: vec![RouteRateLimitRule {
                selector: RateLimitRouteSelector {
                    methods: vec!["GET".into()],
                    path_template: Some("/ledger/{ledger_id}/entries/{entry_id}".into()),
                    operation_id: None,
                },
                policy: policy("ledger-read", OperationClass::PublicRead),
            }],
        };

        let resolved = table
            .resolve(&RouteRateLimitRequest {
                method: "GET",
                path: "/ledger/abc/entries/42",
                route_template: None,
                operation_id: None,
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.policy.policy_id, "ledger-read");
    }

    #[test]
    fn operation_id_beats_a_less_specific_path_rule() {
        let table = RouteRateLimitTable {
            default_policy: None,
            routes: vec![
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["POST".into()],
                        path_template: Some("/jobs/{job_id}".into()),
                        operation_id: None,
                    },
                    policy: policy("job-path", OperationClass::Mutation),
                },
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["POST".into()],
                        path_template: None,
                        operation_id: Some("jobs.retry".into()),
                    },
                    policy: policy("job-retry", OperationClass::JobAdmission),
                },
            ],
        };

        let resolved = table
            .resolve(&RouteRateLimitRequest {
                method: "POST",
                path: "/jobs/123",
                route_template: Some("/jobs/{job_id}"),
                operation_id: Some("jobs.retry"),
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.policy.policy_id, "job-retry");
    }

    #[test]
    fn equally_specific_matches_fail_closed_as_ambiguous() {
        let selector = RateLimitRouteSelector {
            methods: vec!["GET".into()],
            path_template: Some("/users/{id}".into()),
            operation_id: None,
        };
        let table = RouteRateLimitTable {
            default_policy: None,
            routes: vec![
                RouteRateLimitRule {
                    selector: selector.clone(),
                    policy: policy("users-a", OperationClass::PublicRead),
                },
                RouteRateLimitRule {
                    selector,
                    policy: policy("users-b", OperationClass::PublicRead),
                },
            ],
        };

        assert!(matches!(
            table.resolve(&RouteRateLimitRequest {
                method: "GET",
                path: "/users/123",
                route_template: None,
                operation_id: None,
            }),
            Err(RouteRateLimitResolutionError::Ambiguous { .. })
        ));
        assert!(
            table
                .validate()
                .iter()
                .any(|v| v.code == "duplicate-route-selector")
        );
    }

    #[test]
    fn duplicate_selector_detection_ignores_method_order() {
        let table = RouteRateLimitTable {
            default_policy: None,
            routes: vec![
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["GET".into(), "POST".into()],
                        path_template: Some("/search".into()),
                        operation_id: None,
                    },
                    policy: policy("search-a", OperationClass::PublicRead),
                },
                RouteRateLimitRule {
                    selector: RateLimitRouteSelector {
                        methods: vec!["POST".into(), "GET".into()],
                        path_template: Some("/search".into()),
                        operation_id: None,
                    },
                    policy: policy("search-b", OperationClass::PublicRead),
                },
            ],
        };

        assert!(
            table
                .validate()
                .iter()
                .any(|v| v.code == "duplicate-route-selector")
        );
    }

    #[test]
    fn route_without_override_uses_default_policy() {
        let table = RouteRateLimitTable {
            default_policy: Some(policy("default", OperationClass::PublicRead)),
            routes: Vec::new(),
        };
        let resolved = table
            .resolve(&RouteRateLimitRequest {
                method: "GET",
                path: "/docs",
                route_template: Some("/docs"),
                operation_id: Some("docs.read"),
            })
            .unwrap()
            .unwrap();
        assert_eq!(resolved.policy.policy_id, "default");
        assert_eq!(resolved.source, RouteRateLimitPolicySource::Default);
    }
}
