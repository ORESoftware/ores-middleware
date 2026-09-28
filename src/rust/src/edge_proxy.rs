use serde::{Deserialize, Serialize};

use crate::runtime_artifact::MiddlewareProvider;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeProxyKind {
    Caddy,
    Nginx,
    Haproxy,
}

impl EdgeProxyKind {
    #[must_use]
    pub const fn middleware_provider(self) -> MiddlewareProvider {
        return match self {
            Self::Caddy => MiddlewareProvider::Caddy,
            Self::Nginx => MiddlewareProvider::Nginx,
            Self::Haproxy => MiddlewareProvider::Haproxy,
        };
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EdgeProxyCapabilities {
    pub automatic_tls: bool,
    pub runtime_mutation: bool,
    pub graceful_generation_reload: bool,
    pub static_file_serving: bool,
    pub layer4_proxying: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EdgeProxyGeneration {
    pub generation_id: String,
    pub application_generation_sha256: String,
    pub rendered_config_sha256: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeProxyLifecyclePhase {
    Prepared,
    Validated,
    Staged,
    Active,
    Draining,
    Retired,
    RolledBack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EdgeProxyLifecycleReceipt {
    pub proxy: EdgeProxyKind,
    pub generation: EdgeProxyGeneration,
    pub phase: EdgeProxyLifecyclePhase,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EdgeProxyAdapterError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for EdgeProxyAdapterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(formatter, "{} ({})", self.message, self.code);
    }
}

impl std::error::Error for EdgeProxyAdapterError {}

/// Provider-specific edge lifecycle behind one ORES contract.
///
/// Implementations may use Caddy's admin API, NGINX generation reloads, or
/// HAProxy runtime/config APIs. They must not reinterpret application routing or
/// middleware semantics. `application_generation_sha256` is immutable evidence
/// tying every proxy state transition to the same ORES route/middleware/worker
/// generation.
pub trait EdgeProxyAdapter: Send + Sync {
    fn kind(&self) -> EdgeProxyKind;

    fn capabilities(&self) -> EdgeProxyCapabilities;

    fn validate(
        &self,
        generation: &EdgeProxyGeneration,
    ) -> Result<EdgeProxyLifecycleReceipt, EdgeProxyAdapterError>;

    fn stage(
        &self,
        validated: &EdgeProxyLifecycleReceipt,
    ) -> Result<EdgeProxyLifecycleReceipt, EdgeProxyAdapterError>;

    fn activate(
        &self,
        staged: &EdgeProxyLifecycleReceipt,
    ) -> Result<EdgeProxyLifecycleReceipt, EdgeProxyAdapterError>;

    fn drain(
        &self,
        active: &EdgeProxyLifecycleReceipt,
    ) -> Result<EdgeProxyLifecycleReceipt, EdgeProxyAdapterError>;

    fn rollback(
        &self,
        candidate: &EdgeProxyLifecycleReceipt,
        previous_active: &EdgeProxyLifecycleReceipt,
    ) -> Result<EdgeProxyLifecycleReceipt, EdgeProxyAdapterError>;

    fn healthy(&self, active: &EdgeProxyLifecycleReceipt) -> Result<bool, EdgeProxyAdapterError>;
}

/// Shared fail-closed lifecycle guards used by concrete adapters before they
/// invoke provider-specific control mechanisms.
pub fn require_phase(
    receipt: &EdgeProxyLifecycleReceipt,
    expected: EdgeProxyLifecyclePhase,
) -> Result<(), EdgeProxyAdapterError> {
    if receipt.phase != expected {
        return Err(EdgeProxyAdapterError {
            code: "invalid_edge_proxy_phase",
            message: format!(
                "edge proxy lifecycle expected {expected:?}, received {:?}",
                receipt.phase
            ),
        });
    }

    return Ok(());
}

pub fn require_same_application_generation(
    left: &EdgeProxyLifecycleReceipt,
    right: &EdgeProxyLifecycleReceipt,
) -> Result<(), EdgeProxyAdapterError> {
    if left.generation.application_generation_sha256
        != right.generation.application_generation_sha256
    {
        return Err(EdgeProxyAdapterError {
            code: "application_generation_mismatch",
            message: "proxy lifecycle receipts refer to different ORES application generations"
                .to_owned(),
        });
    }

    return Ok(());
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct EdgeProxyDescriptor {
    pub kind: EdgeProxyKind,
    pub capabilities: EdgeProxyCapabilities,
}

#[must_use]
pub const fn edge_proxy_descriptor(kind: EdgeProxyKind) -> EdgeProxyDescriptor {
    let capabilities = match kind {
        EdgeProxyKind::Caddy => EdgeProxyCapabilities {
            automatic_tls: true,
            runtime_mutation: true,
            graceful_generation_reload: true,
            static_file_serving: true,
            layer4_proxying: false,
        },
        EdgeProxyKind::Nginx => EdgeProxyCapabilities {
            automatic_tls: false,
            runtime_mutation: false,
            graceful_generation_reload: true,
            static_file_serving: true,
            layer4_proxying: true,
        },
        EdgeProxyKind::Haproxy => EdgeProxyCapabilities {
            automatic_tls: false,
            runtime_mutation: true,
            graceful_generation_reload: true,
            static_file_serving: false,
            layer4_proxying: true,
        },
    };

    return EdgeProxyDescriptor { kind, capabilities };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(id: &str, application_digest: &str) -> EdgeProxyGeneration {
        return EdgeProxyGeneration {
            generation_id: id.to_owned(),
            application_generation_sha256: application_digest.to_owned(),
            rendered_config_sha256:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
        };
    }

    fn receipt(
        kind: EdgeProxyKind,
        generation: EdgeProxyGeneration,
        phase: EdgeProxyLifecyclePhase,
    ) -> EdgeProxyLifecycleReceipt {
        return EdgeProxyLifecycleReceipt {
            proxy: kind,
            generation,
            phase,
        };
    }

    #[test]
    fn every_proxy_maps_to_a_provider_overlay_name() {
        assert_eq!(
            EdgeProxyKind::Caddy.middleware_provider(),
            MiddlewareProvider::Caddy
        );
        assert_eq!(
            EdgeProxyKind::Nginx.middleware_provider(),
            MiddlewareProvider::Nginx
        );
        assert_eq!(
            EdgeProxyKind::Haproxy.middleware_provider(),
            MiddlewareProvider::Haproxy
        );
    }

    #[test]
    fn swapping_proxy_does_not_change_application_generation_identity() {
        let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let caddy = receipt(
            EdgeProxyKind::Caddy,
            generation("edge-caddy-12", digest),
            EdgeProxyLifecyclePhase::Active,
        );
        let haproxy = receipt(
            EdgeProxyKind::Haproxy,
            generation("edge-haproxy-13", digest),
            EdgeProxyLifecyclePhase::Staged,
        );

        assert!(require_same_application_generation(&caddy, &haproxy).is_ok());
    }

    #[test]
    fn different_application_generation_fails_closed() {
        let active = receipt(
            EdgeProxyKind::Nginx,
            generation(
                "edge-nginx-4",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            EdgeProxyLifecyclePhase::Active,
        );
        let staged = receipt(
            EdgeProxyKind::Caddy,
            generation(
                "edge-caddy-5",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
            EdgeProxyLifecyclePhase::Staged,
        );

        let error = require_same_application_generation(&active, &staged)
            .expect_err("different application generations must not be swapped");
        assert_eq!(error.code, "application_generation_mismatch");
    }

    #[test]
    fn descriptor_keeps_proxy_strengths_explicit() {
        let caddy = edge_proxy_descriptor(EdgeProxyKind::Caddy);
        let nginx = edge_proxy_descriptor(EdgeProxyKind::Nginx);
        let haproxy = edge_proxy_descriptor(EdgeProxyKind::Haproxy);

        assert!(caddy.capabilities.automatic_tls);
        assert!(nginx.capabilities.static_file_serving);
        assert!(haproxy.capabilities.runtime_mutation);
        assert!(haproxy.capabilities.layer4_proxying);
    }
}
