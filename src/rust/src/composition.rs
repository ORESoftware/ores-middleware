use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

/// Severity for a consumer-authored middleware composition rule.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderIssueSeverity {
    #[default]
    Error,
    Advisory,
}

/// One ordering relationship selected by the consuming service.
///
/// `ores-middleware` does not install any global rules here. Consumers may use
/// arbitrary names, including middleware that is not implemented by this crate.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareOrderingRule {
    pub before: String,
    pub after: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub severity: OrderIssueSeverity,
    /// When false, evaluate this rule only when both named stages are present.
    #[serde(default)]
    pub require_both: bool,
}

impl MiddlewareOrderingRule {
    #[must_use]
    pub fn before(
        before: impl Into<String>,
        after: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            before: before.into(),
            after: after.into(),
            code: code.into(),
            message: message.into(),
            severity: OrderIssueSeverity::Error,
            require_both: false,
        }
    }

    #[must_use]
    pub const fn advisory(mut self) -> Self {
        self.severity = OrderIssueSeverity::Advisory;
        self
    }

    #[must_use]
    pub const fn require_both(mut self) -> Self {
        self.require_both = true;
        self
    }
}

/// Consumer-owned middleware composition policy.
///
/// Empty/default policy accepts every selection and order. A service opts into
/// only the rules meaningful for that service or route.
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareOrderPolicy {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    /// Only stages named here are required to be unique. Other stage names may
    /// intentionally occur multiple times.
    #[serde(default)]
    pub unique: Vec<String>,
    #[serde(default)]
    pub first: Option<String>,
    #[serde(default)]
    pub rules: Vec<MiddlewareOrderingRule>,
}

impl MiddlewareOrderPolicy {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn require(mut self, stage: impl Into<String>) -> Self {
        self.required.push(stage.into());
        self
    }

    #[must_use]
    pub fn forbid(mut self, stage: impl Into<String>) -> Self {
        self.forbidden.push(stage.into());
        self
    }

    #[must_use]
    pub fn unique(mut self, stage: impl Into<String>) -> Self {
        self.unique.push(stage.into());
        self
    }

    #[must_use]
    pub fn require_first(mut self, stage: impl Into<String>) -> Self {
        self.first = Some(stage.into());
        self
    }

    #[must_use]
    pub fn rule(mut self, rule: MiddlewareOrderingRule) -> Self {
        self.rules.push(rule);
        self
    }
}

/// Serializable declaration of the exact middleware sequence selected by a
/// consumer plus the consumer's own validation policy.
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareCompositionPlan {
    /// Exact request-stage sequence expected at runtime. Names are open strings.
    #[serde(default)]
    pub stages: Vec<String>,
    /// Constraints authored by the consumer. Empty means unconstrained.
    #[serde(default)]
    pub policy: MiddlewareOrderPolicy,
}

impl MiddlewareCompositionPlan {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn stage(mut self, stage: impl Into<String>) -> Self {
        self.stages.push(stage.into());
        self
    }

    #[must_use]
    pub fn with_policy(mut self, policy: MiddlewareOrderPolicy) -> Self {
        self.policy = policy;
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareOrderIssue {
    pub code: String,
    pub message: String,
    pub severity: OrderIssueSeverity,
    pub stage: Option<String>,
}

fn valid_order_rule(rule: &MiddlewareOrderingRule) -> bool {
    !rule.before.trim().is_empty()
        && !rule.after.trim().is_empty()
        && !rule.code.trim().is_empty()
        && !rule.message.trim().is_empty()
        && rule.before != rule.after
}

fn validate_policy_shape(policy: &MiddlewareOrderPolicy) -> Vec<MiddlewareOrderIssue> {
    let blank_selectors = policy
        .required
        .iter()
        .chain(&policy.forbidden)
        .chain(&policy.unique)
        .filter_map(|stage| {
            stage.trim().is_empty().then_some(MiddlewareOrderIssue {
                code: "blank-policy-stage-name".into(),
                message: "consumer middleware policy stage names must not be blank".into(),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
        });

    let blank_first = policy.first.as_ref().and_then(|stage| {
        stage.trim().is_empty().then_some(MiddlewareOrderIssue {
            code: "blank-policy-stage-name".into(),
            message: "consumer middleware policy first-stage name must not be blank".into(),
            severity: OrderIssueSeverity::Error,
            stage: Some(stage.clone()),
        })
    });

    let malformed_rules = policy.rules.iter().filter_map(|rule| {
        (!valid_order_rule(rule)).then_some(MiddlewareOrderIssue {
            code: "invalid-order-rule".into(),
            message: "order rules require non-empty, distinct before/after names, a non-empty code, and a non-empty message"
                .into(),
            severity: OrderIssueSeverity::Error,
            stage: None,
        })
    });

    let required = policy
        .required
        .iter()
        .filter(|stage| !stage.trim().is_empty())
        .cloned()
        .collect::<HashSet<_>>();
    let forbidden = policy
        .forbidden
        .iter()
        .filter(|stage| !stage.trim().is_empty())
        .cloned()
        .collect::<HashSet<_>>();
    let mut contradictory = required
        .intersection(&forbidden)
        .cloned()
        .collect::<HashSet<_>>();
    if let Some(first) = policy
        .first
        .as_ref()
        .filter(|stage| !stage.trim().is_empty())
        && forbidden.contains(first)
    {
        contradictory.insert(first.clone());
    }
    let mut contradictory = contradictory.into_iter().collect::<Vec<_>>();
    contradictory.sort();
    let contradictions = contradictory.into_iter().map(|stage| MiddlewareOrderIssue {
        code: "contradictory-stage-policy".into(),
        message: format!(
            "consumer middleware policy both requires/places and forbids stage {stage:?}"
        ),
        severity: OrderIssueSeverity::Error,
        stage: Some(stage),
    });

    blank_selectors
        .chain(blank_first)
        .chain(malformed_rules)
        .chain(contradictions)
        .collect()
}

fn active_order_cycle_issue(
    names: &[String],
    policy: &MiddlewareOrderPolicy,
) -> Option<MiddlewareOrderIssue> {
    let present = names.iter().cloned().collect::<HashSet<_>>();
    let active_edges = policy
        .rules
        .iter()
        .filter(|rule| {
            valid_order_rule(rule)
                && present.contains(&rule.before)
                && present.contains(&rule.after)
        })
        .map(|rule| (rule.before.clone(), rule.after.clone()))
        .collect::<HashSet<_>>();

    if active_edges.is_empty() {
        return None;
    }

    let mut adjacency = HashMap::<String, Vec<String>>::new();
    let mut indegree = HashMap::<String, usize>::new();
    for (before, after) in active_edges {
        indegree.entry(before.clone()).or_insert(0);
        *indegree.entry(after.clone()).or_insert(0) += 1;
        adjacency.entry(before).or_default().push(after);
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(stage, degree)| (*degree == 0).then_some(stage.clone()))
        .collect::<VecDeque<_>>();
    let mut visited = 0usize;
    while let Some(stage) = queue.pop_front() {
        visited += 1;
        if let Some(next) = adjacency.get(&stage) {
            for after in next {
                let degree = indegree
                    .get_mut(after)
                    .expect("active ordering graph contains every target node");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(after.clone());
                }
            }
        }
    }

    if visited == indegree.len() {
        return None;
    }

    let mut cycle_candidates = indegree
        .into_iter()
        .filter_map(|(stage, degree)| (degree > 0).then_some(stage))
        .collect::<Vec<_>>();
    cycle_candidates.sort();
    Some(MiddlewareOrderIssue {
        code: "consumer-order-cycle".into(),
        message: format!(
            "active consumer middleware ordering rules contain a cycle involving {cycle_candidates:?}"
        ),
        severity: OrderIssueSeverity::Error,
        stage: None,
    })
}

/// Validate a concrete middleware sequence against policy supplied by the
/// consuming service.
///
/// The default policy imposes no selection, order, uniqueness, or first-stage
/// requirement. When duplicate stage names are allowed, a `before -> after`
/// rule requires the last `before` occurrence to precede the first `after`
/// occurrence so interleaving cannot silently satisfy the rule.
///
/// Policy declarations are validated at the same boundary: blank selectors,
/// contradictory required/forbidden declarations, malformed rules, and active
/// ordering cycles are returned as explicit issues instead of being left to
/// incidental runtime ordering failures.
pub fn validate_consumer_middleware_order<S>(
    stages: &[S],
    policy: &MiddlewareOrderPolicy,
) -> Vec<MiddlewareOrderIssue>
where
    S: AsRef<str>,
{
    let names = stages
        .iter()
        .map(|stage| stage.as_ref().to_owned())
        .collect::<Vec<_>>();
    let present = names.iter().cloned().collect::<HashSet<_>>();
    let positions = names.iter().enumerate().fold(
        HashMap::<String, Vec<usize>>::new(),
        |mut map, (index, name)| {
            map.entry(name.clone()).or_default().push(index);
            map
        },
    );

    let required = policy
        .required
        .iter()
        .filter(|stage| !stage.trim().is_empty())
        .filter_map(|stage| {
            (!present.contains(stage)).then_some(MiddlewareOrderIssue {
                code: "required-stage-missing".into(),
                message: format!("consumer policy requires middleware stage {stage:?}"),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
        });

    let forbidden = policy
        .forbidden
        .iter()
        .filter(|stage| !stage.trim().is_empty())
        .filter_map(|stage| {
            present.contains(stage).then_some(MiddlewareOrderIssue {
                code: "forbidden-stage-present".into(),
                message: format!("consumer policy forbids middleware stage {stage:?}"),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
        });

    let unique = policy
        .unique
        .iter()
        .filter(|stage| !stage.trim().is_empty())
        .filter_map(|stage| {
            positions
                .get(stage)
                .is_some_and(|indexes| indexes.len() > 1)
                .then_some(MiddlewareOrderIssue {
                    code: "consumer-unique-stage-duplicated".into(),
                    message: format!(
                        "consumer policy requires stage {stage:?} to occur at most once"
                    ),
                    severity: OrderIssueSeverity::Error,
                    stage: Some(stage.clone()),
                })
        });

    let first = policy
        .first
        .as_ref()
        .filter(|stage| !stage.trim().is_empty())
        .and_then(|stage| {
            (names.first() != Some(stage)).then_some(MiddlewareOrderIssue {
                code: "consumer-first-stage-mismatch".into(),
                message: format!("consumer policy requires {stage:?} to be the first stage"),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
        });

    let order = policy
        .rules
        .iter()
        .filter(|rule| valid_order_rule(rule))
        .filter_map(|rule| {
            let before = positions
                .get(&rule.before)
                .and_then(|value| value.last())
                .copied();
            let after = positions
                .get(&rule.after)
                .and_then(|value| value.first())
                .copied();
            match (before, after) {
                (Some(left), Some(right)) if left < right => None,
                (Some(_), Some(_)) => Some(MiddlewareOrderIssue {
                    code: rule.code.clone(),
                    message: rule.message.clone(),
                    severity: rule.severity,
                    stage: None,
                }),
                _ if rule.require_both => Some(MiddlewareOrderIssue {
                    code: rule.code.clone(),
                    message: rule.message.clone(),
                    severity: rule.severity,
                    stage: None,
                }),
                _ => None,
            }
        });

    validate_policy_shape(policy)
        .into_iter()
        .chain(required)
        .chain(forbidden)
        .chain(unique)
        .chain(first)
        .chain(order)
        .chain(active_order_cycle_issue(&names, policy))
        .collect()
}

/// Validate only plan-owned stage declarations. Policy shape is checked by
/// `validate_consumer_middleware_order(...)` so callers receive one copy of each
/// policy issue.
fn validate_plan_shape(plan: &MiddlewareCompositionPlan) -> Vec<MiddlewareOrderIssue> {
    plan.stages
        .iter()
        .filter_map(|stage| {
            stage.trim().is_empty().then_some(MiddlewareOrderIssue {
                code: "blank-stage-name".into(),
                message: "middleware stage names must not be blank".into(),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
        })
        .collect()
}

/// Validate the declaration itself before a runtime pipeline is built.
pub fn validate_declared_middleware_plan(
    plan: &MiddlewareCompositionPlan,
) -> Vec<MiddlewareOrderIssue> {
    validate_plan_shape(plan)
        .into_iter()
        .chain(validate_consumer_middleware_order(
            &plan.stages,
            &plan.policy,
        ))
        .collect()
}

/// Verify that the stages actually installed by a runtime match the consumer's
/// declared plan exactly, and apply the policy to the *live* runtime sequence.
///
/// It is important that policy evaluation uses `actual_stages`, not the declared
/// sequence: a drifted runtime must report both the exact-plan mismatch and any
/// security/semantic ordering constraint it violates.
pub fn validate_runtime_middleware_plan<S>(
    actual_stages: &[S],
    plan: &MiddlewareCompositionPlan,
) -> Vec<MiddlewareOrderIssue>
where
    S: AsRef<str>,
{
    let actual = actual_stages
        .iter()
        .map(|stage| stage.as_ref())
        .collect::<Vec<_>>();
    let declared = plan.stages.iter().map(String::as_str).collect::<Vec<_>>();

    let drift = (actual != declared).then_some(MiddlewareOrderIssue {
        code: "runtime-stage-plan-mismatch".into(),
        message: format!(
            "runtime middleware stages {actual:?} do not match declared stages {declared:?}"
        ),
        severity: OrderIssueSeverity::Error,
        stage: None,
    });

    validate_plan_shape(plan)
        .into_iter()
        .chain(validate_consumer_middleware_order(
            actual_stages,
            &plan.policy,
        ))
        .chain(drift)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_accepts_custom_selection_order_and_duplicates() {
        let stages = ["tenant-bootstrap", "custom-auth", "custom-auth", "handler"];
        assert!(
            validate_consumer_middleware_order(&stages, &MiddlewareOrderPolicy::new()).is_empty()
        );
    }

    #[test]
    fn consumer_can_require_only_its_own_relationships() {
        let policy = MiddlewareOrderPolicy::new()
            .require("custom-auth")
            .unique("tenant-rate-limit")
            .rule(MiddlewareOrderingRule::before(
                "custom-auth",
                "tenant-rate-limit",
                "auth-before-tenant-limit",
                "this service derives the rate-limit principal from authenticated identity",
            ));

        let valid = ["request-id", "custom-auth", "tenant-rate-limit", "handler"];
        assert!(validate_consumer_middleware_order(&valid, &policy).is_empty());

        let invalid = ["tenant-rate-limit", "custom-auth", "handler"];
        assert!(
            validate_consumer_middleware_order(&invalid, &policy)
                .iter()
                .any(|issue| issue.code == "auth-before-tenant-limit")
        );
    }

    #[test]
    fn duplicate_stage_cannot_interleave_across_an_ordering_boundary() {
        let policy = MiddlewareOrderPolicy::new().rule(MiddlewareOrderingRule::before(
            "auth",
            "rate-limit",
            "auth-before-limit",
            "every auth layer must execute before the limiting boundary",
        ));

        let valid = ["auth", "auth", "rate-limit", "handler"];
        assert!(validate_consumer_middleware_order(&valid, &policy).is_empty());

        let interleaved = ["auth", "rate-limit", "auth", "handler"];
        assert!(
            validate_consumer_middleware_order(&interleaved, &policy)
                .iter()
                .any(|issue| issue.code == "auth-before-limit")
        );
    }

    #[test]
    fn optional_order_rule_does_not_require_optional_stage() {
        let policy = MiddlewareOrderPolicy::new().rule(MiddlewareOrderingRule::before(
            "request-id",
            "optional-metrics",
            "request-id-before-metrics",
            "when metrics is enabled it observes the request id",
        ));
        let stages = ["request-id", "handler"];
        assert!(validate_consumer_middleware_order(&stages, &policy).is_empty());
    }

    #[test]
    fn explicit_presence_and_first_stage_requirements_fail_closed() {
        let policy = MiddlewareOrderPolicy::new()
            .require("auth")
            .forbid("test-bypass")
            .require_first("recovery");
        let stages = ["request-id", "test-bypass", "handler"];
        let issues = validate_consumer_middleware_order(&stages, &policy);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "required-stage-missing")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "forbidden-stage-present")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "consumer-first-stage-mismatch")
        );
    }

    #[test]
    fn serialized_rules_default_to_error_and_optional_presence() {
        let rule: MiddlewareOrderingRule = serde_json::from_str(
            r#"{"before":"auth","after":"handler","code":"auth-first","message":"auth before handler"}"#,
        )
        .expect("rule without optional fields");
        assert_eq!(rule.severity, OrderIssueSeverity::Error);
        assert!(!rule.require_both);
    }

    #[test]
    fn policy_rejects_blank_selectors_and_contradictions() {
        let policy = MiddlewareOrderPolicy::new()
            .require("auth")
            .require(" ")
            .forbid("auth")
            .unique("")
            .require_first("auth");
        let issues = validate_consumer_middleware_order(&["auth"], &policy);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "blank-policy-stage-name")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "contradictory-stage-policy")
        );
    }

    #[test]
    fn active_order_cycle_is_reported_explicitly() {
        let policy = MiddlewareOrderPolicy::new()
            .rule(MiddlewareOrderingRule::before(
                "auth",
                "limit",
                "auth-before-limit",
                "auth before limit",
            ))
            .rule(MiddlewareOrderingRule::before(
                "limit",
                "auth",
                "limit-before-auth",
                "limit before auth",
            ));
        let issues = validate_consumer_middleware_order(&["auth", "limit"], &policy);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "consumer-order-cycle")
        );
    }

    #[test]
    fn optional_absent_rules_do_not_create_an_active_cycle() {
        let policy = MiddlewareOrderPolicy::new()
            .rule(MiddlewareOrderingRule::before(
                "auth",
                "optional-metrics",
                "auth-before-metrics",
                "auth before metrics",
            ))
            .rule(MiddlewareOrderingRule::before(
                "optional-metrics",
                "auth",
                "metrics-before-auth",
                "metrics before auth",
            ));
        let issues = validate_consumer_middleware_order(&["auth", "handler"], &policy);

        assert!(
            issues
                .iter()
                .all(|issue| issue.code != "consumer-order-cycle")
        );
    }

    #[test]
    fn malformed_rule_requires_diagnostic_message() {
        let policy = MiddlewareOrderPolicy::new().rule(MiddlewareOrderingRule::before(
            "auth", "handler", "auth-first", " ",
        ));
        let issues = validate_consumer_middleware_order(&["auth", "handler"], &policy);
        assert!(issues.iter().any(|issue| issue.code == "invalid-order-rule"));
        assert!(issues.iter().all(|issue| issue.code != "auth-first"));
    }

    #[test]
    fn runtime_plan_detects_exact_order_drift_without_legacy_defaults() {
        let plan = MiddlewareCompositionPlan::new()
            .stage("custom-auth")
            .stage("custom-limit")
            .stage("handler")
            .with_policy(MiddlewareOrderPolicy::new().rule(
                MiddlewareOrderingRule::before(
                    "custom-auth",
                    "custom-limit",
                    "auth-before-limit",
                    "consumer-selected identity dependency",
                ),
            ));

        assert!(
            validate_runtime_middleware_plan(
                &["custom-auth", "custom-limit", "handler"],
                &plan,
            )
            .is_empty()
        );

        let issues =
            validate_runtime_middleware_plan(&["custom-limit", "custom-auth", "handler"], &plan);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "runtime-stage-plan-mismatch")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "auth-before-limit")
        );
    }

    #[test]
    fn runtime_plan_applies_presence_rules_to_live_stages() {
        let plan = MiddlewareCompositionPlan::new()
            .stage("auth")
            .stage("handler")
            .with_policy(MiddlewareOrderPolicy::new().require("auth"));

        let issues = validate_runtime_middleware_plan(&["handler"], &plan);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "runtime-stage-plan-mismatch")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "required-stage-missing")
        );
    }

    #[test]
    fn runtime_validation_does_not_duplicate_declared_policy_issues() {
        let plan = MiddlewareCompositionPlan::new()
            .stage("handler")
            .with_policy(MiddlewareOrderPolicy::new().require("auth"));

        let issues = validate_runtime_middleware_plan(&["handler"], &plan);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.code == "required-stage-missing")
                .count(),
            1
        );
    }

    #[test]
    fn declared_plan_rejects_blank_names_and_self_rules() {
        let plan = MiddlewareCompositionPlan {
            stages: vec!["request-id".into(), " ".into()],
            policy: MiddlewareOrderPolicy::new().rule(MiddlewareOrderingRule::before(
                "auth",
                "auth",
                "self-rule",
                "invalid",
            )),
        };
        let issues = validate_declared_middleware_plan(&plan);
        assert!(issues.iter().any(|issue| issue.code == "blank-stage-name"));
        assert!(issues.iter().any(|issue| issue.code == "invalid-order-rule"));
    }
}
