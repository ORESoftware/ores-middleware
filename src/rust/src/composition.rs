use std::collections::{HashMap, HashSet};

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
    /// When false, the rule is evaluated only when both named stages are present.
    /// This lets one policy describe optional middleware without implicitly
    /// requiring it. Presence is controlled independently by `required`.
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
/// Empty/default policy accepts every selection and order. Nothing in this type
/// imports the repository's reviewed reference profile: a service opts into only
/// the rules that are meaningful for that service/route.
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareOrderPolicy {
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    /// Only stages named here are required to be unique. Other stage names may
    /// intentionally occur multiple times (for example two independent auth
    /// providers or two observation layers).
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MiddlewareOrderIssue {
    pub code: String,
    pub message: String,
    pub severity: OrderIssueSeverity,
    pub stage: Option<String>,
}

/// Validate a concrete middleware plan against policy supplied by the consuming
/// service. Stage names are intentionally open strings so custom/org-specific
/// middleware can participate without changes to `ores-middleware`.
///
/// The default policy imposes no selection, order, uniqueness, or first-stage
/// requirement. When duplicate stage names are allowed, a `before -> after`
/// rule requires the *last* `before` occurrence to precede the *first* `after`
/// occurrence, preventing an interleaved duplicate from silently satisfying the
/// relationship.
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
    let positions = names
        .iter()
        .enumerate()
        .fold(HashMap::<String, Vec<usize>>::new(), |mut map, (index, name)| {
            map.entry(name.clone()).or_default().push(index);
            map
        });

    let required = policy.required.iter().filter_map(|stage| {
        (!present.contains(stage)).then_some(MiddlewareOrderIssue {
            code: "required-stage-missing".into(),
            message: format!("consumer policy requires middleware stage {stage:?}"),
            severity: OrderIssueSeverity::Error,
            stage: Some(stage.clone()),
        })
    });

    let forbidden = policy.forbidden.iter().filter_map(|stage| {
        present.contains(stage).then_some(MiddlewareOrderIssue {
            code: "forbidden-stage-present".into(),
            message: format!("consumer policy forbids middleware stage {stage:?}"),
            severity: OrderIssueSeverity::Error,
            stage: Some(stage.clone()),
        })
    });

    let unique = policy.unique.iter().filter_map(|stage| {
        positions
            .get(stage)
            .is_some_and(|indexes| indexes.len() > 1)
            .then_some(MiddlewareOrderIssue {
                code: "consumer-unique-stage-duplicated".into(),
                message: format!("consumer policy requires stage {stage:?} to occur at most once"),
                severity: OrderIssueSeverity::Error,
                stage: Some(stage.clone()),
            })
    });

    let first = policy.first.as_ref().and_then(|stage| {
        (names.first() != Some(stage)).then_some(MiddlewareOrderIssue {
            code: "consumer-first-stage-mismatch".into(),
            message: format!("consumer policy requires {stage:?} to be the first stage"),
            severity: OrderIssueSeverity::Error,
            stage: Some(stage.clone()),
        })
    });

    let order = policy.rules.iter().filter_map(|rule| {
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

    required
        .chain(forbidden)
        .chain(unique)
        .chain(first)
        .chain(order)
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
        assert!(issues.iter().any(|issue| issue.code == "required-stage-missing"));
        assert!(issues.iter().any(|issue| issue.code == "forbidden-stage-present"));
        assert!(issues.iter().any(|issue| issue.code == "consumer-first-stage-mismatch"));
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
}
