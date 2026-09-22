use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiddlewarePlacement {
    Edge,
    Transport,
    Application,
}

/// Capability class for one middleware execution unit.
///
/// This is deliberately about the API surface exposed to middleware, not where
/// the process happens to run. Local `ores-stack dev` P2 can therefore execute
/// an edge profile while P3 remains a full backend process.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareExecutionProfile {
    /// Broadest-portability P2 profile. Middleware can inspect request metadata,
    /// mutate request headers, make host-provided outbound fetches, and either
    /// continue/route or short-circuit. It does not receive the downstream
    /// response path or native backend resources.
    EdgeMinimal,
    /// Fetch-style P2 profile. Adds downstream response access to the portable
    /// request/header/fetch surface, so P2 must remain in the data path.
    EdgeFetch,
    /// Full P3/standalone backend profile. Native server resources are allowed
    /// and the middleware executes in the application/Lambda process.
    Backend,
}

impl MiddlewareExecutionProfile {
    #[must_use]
    pub const fn is_p2_compatible(self) -> bool {
        matches!(self, Self::EdgeMinimal | Self::EdgeFetch)
    }

    #[must_use]
    pub const fn is_p3_backend(self) -> bool {
        matches!(self, Self::Backend)
    }

    /// `edge_minimal` may hand an admitted connection/request off after the
    /// request-side decision because it has no response-side contract.
    #[must_use]
    pub const fn permits_connection_handoff(self) -> bool {
        matches!(self, Self::EdgeMinimal)
    }

    /// Fetch-style and backend middleware participate in the response path.
    #[must_use]
    pub const fn has_response_access(self) -> bool {
        matches!(self, Self::EdgeFetch | Self::Backend)
    }

    #[must_use]
    pub const fn allows_request_header_mutation(self) -> bool {
        true
    }

    /// All three profiles may use a host-provided outbound HTTP fetch client;
    /// the edge profiles intentionally expose no lower-level socket API.
    #[must_use]
    pub const fn allows_outbound_fetch(self) -> bool {
        true
    }

    #[must_use]
    pub const fn allows_native_backend_access(self) -> bool {
        matches!(self, Self::Backend)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiddlewareExecutionTarget {
    CloudflareWorker,
    KubernetesIngress,
    LoadBalancer,
    ServiceMesh,
    WebServer,
    ApiServer,
    LambdaFunction,
}

impl MiddlewareExecutionTarget {
    #[must_use]
    pub const fn placement(self) -> MiddlewarePlacement {
        match self {
            Self::CloudflareWorker | Self::KubernetesIngress | Self::LoadBalancer => {
                MiddlewarePlacement::Edge
            }
            Self::ServiceMesh => MiddlewarePlacement::Transport,
            Self::WebServer | Self::ApiServer | Self::LambdaFunction => {
                MiddlewarePlacement::Application
            }
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MiddlewareCapabilities {
    pub placements: Vec<MiddlewarePlacement>,
    #[serde(default)]
    pub requires_request_body: bool,
    #[serde(default)]
    pub requires_authenticated_identity: bool,
    #[serde(default)]
    pub requires_database: bool,
}

impl MiddlewareCapabilities {
    #[must_use]
    pub fn portable(placements: impl IntoIterator<Item = MiddlewarePlacement>) -> Self {
        Self {
            placements: placements.into_iter().collect(),
            requires_request_body: false,
            requires_authenticated_identity: false,
            requires_database: false,
        }
    }

    #[must_use]
    pub fn validate_target(
        &self,
        target: MiddlewareExecutionTarget,
    ) -> Vec<MiddlewarePlacementViolation> {
        let mut violations = Vec::new();
        let placement = target.placement();

        if !self.placements.contains(&placement) {
            violations.push(MiddlewarePlacementViolation {
                code: "unsupported-placement",
                message: "middleware capability does not permit this execution placement",
            });
        }

        if self.requires_database && placement != MiddlewarePlacement::Application {
            violations.push(MiddlewarePlacementViolation {
                code: "database-required-outside-application",
                message: "database-dependent middleware must execute at the application boundary",
            });
        }

        if self.requires_authenticated_identity
            && target == MiddlewareExecutionTarget::CloudflareWorker
        {
            violations.push(MiddlewarePlacementViolation {
                code: "authenticated-identity-at-untrusted-edge",
                message: "authenticated identity middleware must not assume origin identity at the Cloudflare edge",
            });
        }

        violations
    }

    /// Fail closed when a stage requests capabilities that the selected
    /// execution profile intentionally does not expose.
    #[must_use]
    pub fn validate_profile(
        &self,
        profile: MiddlewareExecutionProfile,
    ) -> Vec<MiddlewarePlacementViolation> {
        let mut violations = Vec::new();

        if profile == MiddlewareExecutionProfile::EdgeMinimal && self.requires_request_body {
            violations.push(MiddlewarePlacementViolation {
                code: "request-body-outside-edge-minimal",
                message: "edge_minimal middleware may not require request-body access",
            });
        }

        if profile.is_p2_compatible() && self.requires_database {
            violations.push(MiddlewarePlacementViolation {
                code: "database-outside-backend-profile",
                message: "database-dependent middleware requires the backend profile",
            });
        }

        violations
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MiddlewarePlacementViolation {
    pub code: &'static str,
    pub message: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloudflare_ingress_and_lb_are_edge_targets() {
        assert_eq!(
            MiddlewareExecutionTarget::CloudflareWorker.placement(),
            MiddlewarePlacement::Edge
        );
        assert_eq!(
            MiddlewareExecutionTarget::KubernetesIngress.placement(),
            MiddlewarePlacement::Edge
        );
        assert_eq!(
            MiddlewareExecutionTarget::LoadBalancer.placement(),
            MiddlewarePlacement::Edge
        );
    }

    #[test]
    fn lambda_functions_are_application_targets() {
        assert_eq!(
            MiddlewareExecutionTarget::LambdaFunction.placement(),
            MiddlewarePlacement::Application
        );
        let database_capability = MiddlewareCapabilities {
            placements: vec![MiddlewarePlacement::Application],
            requires_request_body: false,
            requires_authenticated_identity: false,
            requires_database: true,
        };
        assert!(
            database_capability
                .validate_target(MiddlewareExecutionTarget::LambdaFunction)
                .is_empty()
        );
    }

    #[test]
    fn database_middleware_is_rejected_at_ingress() {
        let capabilities = MiddlewareCapabilities {
            placements: vec![MiddlewarePlacement::Edge, MiddlewarePlacement::Application],
            requires_request_body: false,
            requires_authenticated_identity: false,
            requires_database: true,
        };
        assert!(
            capabilities
                .validate_target(MiddlewareExecutionTarget::KubernetesIngress)
                .iter()
                .any(|violation| violation.code == "database-required-outside-application")
        );
    }

    #[test]
    fn portable_request_metadata_stage_can_run_at_edge_and_application() {
        let capabilities = MiddlewareCapabilities::portable([
            MiddlewarePlacement::Edge,
            MiddlewarePlacement::Application,
        ]);
        assert!(
            capabilities
                .validate_target(MiddlewareExecutionTarget::LoadBalancer)
                .is_empty()
        );
        assert!(
            capabilities
                .validate_target(MiddlewareExecutionTarget::ApiServer)
                .is_empty()
        );
        assert!(
            capabilities
                .validate_target(MiddlewareExecutionTarget::LambdaFunction)
                .is_empty()
        );
    }

    #[test]
    fn profiles_pin_the_p2_p3_capability_boundary() {
        let minimal = MiddlewareExecutionProfile::EdgeMinimal;
        assert!(minimal.is_p2_compatible());
        assert!(minimal.permits_connection_handoff());
        assert!(!minimal.has_response_access());
        assert!(minimal.allows_request_header_mutation());
        assert!(minimal.allows_outbound_fetch());
        assert!(!minimal.allows_native_backend_access());

        let fetch = MiddlewareExecutionProfile::EdgeFetch;
        assert!(fetch.is_p2_compatible());
        assert!(!fetch.permits_connection_handoff());
        assert!(fetch.has_response_access());
        assert!(!fetch.allows_native_backend_access());

        let backend = MiddlewareExecutionProfile::Backend;
        assert!(!backend.is_p2_compatible());
        assert!(backend.is_p3_backend());
        assert!(backend.has_response_access());
        assert!(backend.allows_native_backend_access());
    }

    #[test]
    fn edge_minimal_rejects_body_and_all_p2_profiles_reject_database_access() {
        let body = MiddlewareCapabilities {
            placements: vec![MiddlewarePlacement::Edge],
            requires_request_body: true,
            requires_authenticated_identity: false,
            requires_database: false,
        };
        assert!(
            body.validate_profile(MiddlewareExecutionProfile::EdgeMinimal)
                .iter()
                .any(|violation| violation.code == "request-body-outside-edge-minimal")
        );
        assert!(
            body.validate_profile(MiddlewareExecutionProfile::EdgeFetch)
                .is_empty()
        );

        let database = MiddlewareCapabilities {
            placements: vec![MiddlewarePlacement::Application],
            requires_request_body: false,
            requires_authenticated_identity: false,
            requires_database: true,
        };
        for profile in [
            MiddlewareExecutionProfile::EdgeMinimal,
            MiddlewareExecutionProfile::EdgeFetch,
        ] {
            assert!(
                database
                    .validate_profile(profile)
                    .iter()
                    .any(|violation| violation.code == "database-outside-backend-profile")
            );
        }
        assert!(
            database
                .validate_profile(MiddlewareExecutionProfile::Backend)
                .is_empty()
        );
    }
}
