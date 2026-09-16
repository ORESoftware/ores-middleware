use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiddlewarePlacement {
    Edge,
    Transport,
    Application,
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
}

impl MiddlewareExecutionTarget {
    #[must_use]
    pub const fn placement(self) -> MiddlewarePlacement {
        match self {
            Self::CloudflareWorker | Self::KubernetesIngress | Self::LoadBalancer => {
                MiddlewarePlacement::Edge
            }
            Self::ServiceMesh => MiddlewarePlacement::Transport,
            Self::WebServer | Self::ApiServer => MiddlewarePlacement::Application,
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

        if self.requires_authenticated_identity && target == MiddlewareExecutionTarget::CloudflareWorker
        {
            violations.push(MiddlewarePlacementViolation {
                code: "authenticated-identity-at-untrusted-edge",
                message: "authenticated identity middleware must not assume origin identity at the Cloudflare edge",
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
    }
}
