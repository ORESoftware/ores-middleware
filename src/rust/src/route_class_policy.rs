use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const ROUTE_CLASS_POLICY_SCHEMA: &str = "ores.middleware.route-class-policies/v1";
pub const MAX_ROUTE_CLASS_POLICIES: usize = 64;
pub const MAX_ROUTE_CLASS_INHERITANCE_DEPTH: usize = 8;

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteClassPolicyRefs {
    pub auth_policy_id: Option<String>,
    pub authorization_policy_id: Option<String>,
    pub rate_limit_policy_id: Option<String>,
    pub cache_policy_id: Option<String>,
    pub timeout_policy_id: Option<String>,
    pub cors_policy_id: Option<String>,
    pub csrf_policy_id: Option<String>,
    pub validation_policy_id: Option<String>,
    pub resilience_policy_id: Option<String>,
    pub idempotency_policy_id: Option<String>,
}

impl RouteClassPolicyRefs {
    fn validate(&self, prefix: &str) -> Vec<RouteClassPolicyViolation> {
        let mut violations = Vec::new();
        for (name, value) in self.iter() {
            if value.is_some_and(|value| !valid_policy_id(value)) {
                violations.push(RouteClassPolicyViolation {
                    code: "invalid-policy-id",
                    path: format!("{prefix}.{name}"),
                    message: "policy references must be bounded stable lowercase identifiers",
                });
            }
        }
        violations
    }

    fn iter(&self) -> [(&'static str, Option<&str>); 10] {
        [
            ("auth_policy_id", self.auth_policy_id.as_deref()),
            (
                "authorization_policy_id",
                self.authorization_policy_id.as_deref(),
            ),
            ("rate_limit_policy_id", self.rate_limit_policy_id.as_deref()),
            ("cache_policy_id", self.cache_policy_id.as_deref()),
            ("timeout_policy_id", self.timeout_policy_id.as_deref()),
            ("cors_policy_id", self.cors_policy_id.as_deref()),
            ("csrf_policy_id", self.csrf_policy_id.as_deref()),
            ("validation_policy_id", self.validation_policy_id.as_deref()),
            ("resilience_policy_id", self.resilience_policy_id.as_deref()),
            (
                "idempotency_policy_id",
                self.idempotency_policy_id.as_deref(),
            ),
        ]
    }

    fn has_security_critical_override(&self) -> bool {
        self.auth_policy_id.is_some()
            || self.authorization_policy_id.is_some()
            || self.csrf_policy_id.is_some()
            || self.validation_policy_id.is_some()
            || self.idempotency_policy_id.is_some()
    }

    fn overlay(&mut self, overrides: &Self) {
        macro_rules! replace_if_some {
            ($field:ident) => {
                if overrides.$field.is_some() {
                    self.$field = overrides.$field.clone();
                }
            };
        }
        replace_if_some!(auth_policy_id);
        replace_if_some!(authorization_policy_id);
        replace_if_some!(rate_limit_policy_id);
        replace_if_some!(cache_policy_id);
        replace_if_some!(timeout_policy_id);
        replace_if_some!(cors_policy_id);
        replace_if_some!(csrf_policy_id);
        replace_if_some!(validation_policy_id);
        replace_if_some!(resilience_policy_id);
        replace_if_some!(idempotency_policy_id);
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecurityOverrideIntent {
    PreserveOrStrengthen,
    Weaken,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SecurityWeakeningException {
    pub exception_id: String,
    pub ticket_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteClassPolicy {
    pub id: String,
    pub extends: Option<String>,
    #[serde(default)]
    pub overrides: RouteClassPolicyRefs,
    pub security_override_intent: Option<SecurityOverrideIntent>,
    pub security_exception: Option<SecurityWeakeningException>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteClassPolicyTable {
    pub schema: String,
    pub default_class_id: String,
    #[serde(default)]
    pub classes: Vec<RouteClassPolicy>,
}

impl Default for RouteClassPolicyTable {
    fn default() -> Self {
        Self {
            schema: ROUTE_CLASS_POLICY_SCHEMA.to_owned(),
            default_class_id: String::new(),
            classes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RouteClassPolicyViolation {
    pub code: &'static str,
    pub path: String,
    pub message: &'static str,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RouteClassPolicyResolutionError {
    InvalidTable {
        violations: Vec<RouteClassPolicyViolation>,
    },
    UnknownRouteClass {
        route_class_id: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedRouteClassPolicy {
    pub route_class_id: String,
    pub lineage: Vec<String>,
    pub policies: RouteClassPolicyRefs,
}

impl RouteClassPolicyTable {
    #[must_use]
    pub fn validate(&self) -> Vec<RouteClassPolicyViolation> {
        let mut violations = Vec::new();

        if self.schema != ROUTE_CLASS_POLICY_SCHEMA {
            violations.push(RouteClassPolicyViolation {
                code: "invalid-route-class-policy-schema",
                path: "schema".into(),
                message: "route class policy schema identifier is not supported",
            });
        }

        if self.classes.is_empty() {
            violations.push(RouteClassPolicyViolation {
                code: "empty-route-class-policy-table",
                path: "classes".into(),
                message: "at least one route class policy is required",
            });
        }
        if self.classes.len() > MAX_ROUTE_CLASS_POLICIES {
            violations.push(RouteClassPolicyViolation {
                code: "too-many-route-class-policies",
                path: "classes".into(),
                message: "route class policy table exceeds its bounded class count",
            });
        }

        if !valid_route_class_id(&self.default_class_id) {
            violations.push(RouteClassPolicyViolation {
                code: "invalid-default-route-class-id",
                path: "default_class_id".into(),
                message: "default_class_id must be a bounded stable lowercase identifier",
            });
        }

        let mut ids = BTreeSet::new();
        for (index, class) in self.classes.iter().enumerate() {
            let prefix = format!("classes[{index}]");

            if !valid_route_class_id(&class.id) {
                violations.push(RouteClassPolicyViolation {
                    code: "invalid-route-class-id",
                    path: format!("{prefix}.id"),
                    message: "route class ids must be bounded stable lowercase identifiers",
                });
            }
            if !ids.insert(class.id.as_str()) {
                violations.push(RouteClassPolicyViolation {
                    code: "duplicate-route-class-id",
                    path: format!("{prefix}.id"),
                    message: "route class ids must be unique within a policy table",
                });
            }

            if class
                .extends
                .as_deref()
                .is_some_and(|value| !valid_route_class_id(value))
            {
                violations.push(RouteClassPolicyViolation {
                    code: "invalid-parent-route-class-id",
                    path: format!("{prefix}.extends"),
                    message: "extends must name a bounded stable route class id",
                });
            }

            violations.extend(class.overrides.validate(&format!("{prefix}.overrides")));

            if let Some(exception) = class.security_exception.as_ref() {
                if !valid_audit_id(&exception.exception_id) {
                    violations.push(RouteClassPolicyViolation {
                        code: "invalid-security-exception-id",
                        path: format!("{prefix}.security_exception.exception_id"),
                        message: "security exception id must be a bounded stable audit identifier",
                    });
                }
                if !valid_audit_id(&exception.ticket_id) {
                    violations.push(RouteClassPolicyViolation {
                        code: "invalid-security-ticket-id",
                        path: format!("{prefix}.security_exception.ticket_id"),
                        message: "security exception ticket id must be a bounded stable audit identifier",
                    });
                }
                if exception.reason.trim().is_empty() || exception.reason.chars().count() > 512 {
                    violations.push(RouteClassPolicyViolation {
                        code: "invalid-security-exception-reason",
                        path: format!("{prefix}.security_exception.reason"),
                        message: "security exception reason must be non-empty and at most 512 characters",
                    });
                }
            }

            let critical = class.overrides.has_security_critical_override();
            if class.extends.is_some() && critical && class.security_override_intent.is_none() {
                violations.push(RouteClassPolicyViolation {
                    code: "security-override-intent-required",
                    path: format!("{prefix}.security_override_intent"),
                    message: "security-critical inherited overrides must declare preserve-or-strengthen or weaken intent",
                });
            }

            match class.security_override_intent {
                Some(SecurityOverrideIntent::Weaken) => {
                    if !critical {
                        violations.push(RouteClassPolicyViolation {
                            code: "security-intent-without-critical-override",
                            path: format!("{prefix}.security_override_intent"),
                            message: "weaken intent requires an actual security-critical override",
                        });
                    }
                    if class.security_exception.is_none() {
                        violations.push(RouteClassPolicyViolation {
                            code: "security-weakening-exception-required",
                            path: format!("{prefix}.security_exception"),
                            message: "security weakening requires explicit reviewed audit metadata",
                        });
                    }
                }
                Some(SecurityOverrideIntent::PreserveOrStrengthen) => {
                    if !critical {
                        violations.push(RouteClassPolicyViolation {
                            code: "security-intent-without-critical-override",
                            path: format!("{prefix}.security_override_intent"),
                            message: "security override intent requires an actual security-critical override",
                        });
                    }
                    if class.security_exception.is_some() {
                        violations.push(RouteClassPolicyViolation {
                            code: "unexpected-security-exception",
                            path: format!("{prefix}.security_exception"),
                            message: "security exceptions are reserved for explicit weakening",
                        });
                    }
                }
                None => {
                    if class.security_exception.is_some() {
                        violations.push(RouteClassPolicyViolation {
                            code: "security-exception-without-intent",
                            path: format!("{prefix}.security_exception"),
                            message: "security exception metadata requires explicit weaken intent",
                        });
                    }
                }
            }
        }

        let by_id = self
            .classes
            .iter()
            .map(|class| (class.id.as_str(), class))
            .collect::<BTreeMap<_, _>>();

        if valid_route_class_id(&self.default_class_id)
            && !by_id.contains_key(self.default_class_id.as_str())
        {
            violations.push(RouteClassPolicyViolation {
                code: "default-route-class-missing",
                path: "default_class_id".into(),
                message: "default_class_id must name a declared route class",
            });
        }

        for (index, class) in self.classes.iter().enumerate() {
            if let Some(parent) = class.extends.as_deref()
                && valid_route_class_id(parent)
                && !by_id.contains_key(parent)
            {
                violations.push(RouteClassPolicyViolation {
                    code: "parent-route-class-missing",
                    path: format!("classes[{index}].extends"),
                    message: "extends must name a declared route class",
                });
            }
        }

        let mut reported_cycles = BTreeSet::<Vec<String>>::new();
        for (index, class) in self.classes.iter().enumerate() {
            if !valid_route_class_id(&class.id) {
                continue;
            }
            let mut seen = BTreeMap::<&str, usize>::new();
            let mut chain = Vec::<&str>::new();
            let mut current = Some(class.id.as_str());
            let mut depth = 0usize;

            while let Some(id) = current {
                if let Some(start) = seen.get(id).copied() {
                    let mut cycle = chain[start..]
                        .iter()
                        .map(|value| (*value).to_owned())
                        .collect::<Vec<_>>();
                    cycle.sort();
                    cycle.dedup();
                    if reported_cycles.insert(cycle) {
                        violations.push(RouteClassPolicyViolation {
                            code: "route-class-inheritance-cycle",
                            path: format!("classes[{index}].extends"),
                            message: "route class inheritance must be acyclic",
                        });
                    }
                    break;
                }

                let Some(node) = by_id.get(id).copied() else {
                    break;
                };
                seen.insert(id, chain.len());
                chain.push(id);
                depth += 1;
                if depth > MAX_ROUTE_CLASS_INHERITANCE_DEPTH {
                    violations.push(RouteClassPolicyViolation {
                        code: "route-class-inheritance-too-deep",
                        path: format!("classes[{index}].extends"),
                        message: "route class inheritance exceeds the bounded depth",
                    });
                    break;
                }
                current = node.extends.as_deref();
            }
        }

        violations
    }

    pub fn resolve(
        &self,
        route_class_id: Option<&str>,
    ) -> Result<ResolvedRouteClassPolicy, RouteClassPolicyResolutionError> {
        let violations = self.validate();
        if !violations.is_empty() {
            return Err(RouteClassPolicyResolutionError::InvalidTable { violations });
        }

        let selected = route_class_id.unwrap_or(self.default_class_id.as_str());
        let by_id = self
            .classes
            .iter()
            .map(|class| (class.id.as_str(), class))
            .collect::<BTreeMap<_, _>>();
        let Some(_) = by_id.get(selected) else {
            return Err(RouteClassPolicyResolutionError::UnknownRouteClass {
                route_class_id: selected.to_owned(),
            });
        };

        let mut lineage = Vec::<&RouteClassPolicy>::new();
        let mut current = Some(selected);
        while let Some(id) = current {
            let node = by_id
                .get(id)
                .copied()
                .expect("validated route class inheritance references only declared classes");
            lineage.push(node);
            current = node.extends.as_deref();
        }
        lineage.reverse();

        let mut policies = RouteClassPolicyRefs::default();
        for node in &lineage {
            policies.overlay(&node.overrides);
        }

        Ok(ResolvedRouteClassPolicy {
            route_class_id: selected.to_owned(),
            lineage: lineage.iter().map(|node| node.id.clone()).collect(),
            policies,
        })
    }
}

fn valid_route_class_id(value: &str) -> bool {
    stable_lower_id(value, 80, false)
}

fn valid_policy_id(value: &str) -> bool {
    stable_lower_id(value, 160, true)
}

fn valid_audit_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_table() -> RouteClassPolicyTable {
        RouteClassPolicyTable {
            schema: ROUTE_CLASS_POLICY_SCHEMA.into(),
            default_class_id: "public-read".into(),
            classes: vec![
                RouteClassPolicy {
                    id: "public-read".into(),
                    extends: None,
                    overrides: RouteClassPolicyRefs {
                        rate_limit_policy_id: Some("public:read".into()),
                        timeout_policy_id: Some("timeout:default".into()),
                        ..Default::default()
                    },
                    security_override_intent: None,
                    security_exception: None,
                },
                RouteClassPolicy {
                    id: "auth-login".into(),
                    extends: Some("public-read".into()),
                    overrides: RouteClassPolicyRefs {
                        auth_policy_id: Some("auth:login".into()),
                        rate_limit_policy_id: Some("auth:login-strict".into()),
                        ..Default::default()
                    },
                    security_override_intent: Some(
                        SecurityOverrideIntent::PreserveOrStrengthen,
                    ),
                    security_exception: None,
                },
            ],
        }
    }

    #[test]
    fn inheritance_overlays_named_policy_refs() {
        let resolved = base_table().resolve(Some("auth-login")).unwrap();
        assert_eq!(resolved.lineage, vec!["public-read", "auth-login"]);
        assert_eq!(
            resolved.policies.timeout_policy_id.as_deref(),
            Some("timeout:default")
        );
        assert_eq!(
            resolved.policies.rate_limit_policy_id.as_deref(),
            Some("auth:login-strict")
        );
        assert_eq!(
            resolved.policies.auth_policy_id.as_deref(),
            Some("auth:login")
        );
    }

    #[test]
    fn default_class_is_used_when_classifier_has_no_route_class() {
        let resolved = base_table().resolve(None).unwrap();
        assert_eq!(resolved.route_class_id, "public-read");
        assert_eq!(resolved.lineage, vec!["public-read"]);
    }

    #[test]
    fn critical_child_override_requires_explicit_intent() {
        let mut table = base_table();
        table.classes[1].security_override_intent = None;
        assert!(table.validate().iter().any(|issue| {
            issue.code == "security-override-intent-required"
        }));
    }

    #[test]
    fn explicit_weakening_requires_audit_metadata() {
        let mut table = base_table();
        table.classes[1].security_override_intent = Some(SecurityOverrideIntent::Weaken);
        assert!(table.validate().iter().any(|issue| {
            issue.code == "security-weakening-exception-required"
        }));
    }

    #[test]
    fn inheritance_cycle_fails_closed() {
        let mut table = base_table();
        table.classes[0].extends = Some("auth-login".into());
        assert!(table.validate().iter().any(|issue| {
            issue.code == "route-class-inheritance-cycle"
        }));
    }
}
