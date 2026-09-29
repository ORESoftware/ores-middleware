use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareArtifactKind {
    JavaScriptModule,
    WebAssemblyModule,
    NativeLibrary,
    BeamModule,
    ProcessRpc,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareRuntimeProfile {
    PortableJavaScript,
    PortableWasm,
    Native,
    IsolatedProcess,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareProvider {
    Generic,
    Cloudflare,
    Caddy,
    Nginx,
    Haproxy,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MiddlewareArtifactDescriptor {
    pub id: String,
    pub language: String,
    pub kind: MiddlewareArtifactKind,
    pub profile: MiddlewareRuntimeProfile,
    pub provider: MiddlewareProvider,
    pub path: String,
    pub semantic_contract_sha256: String,
}

impl MiddlewareArtifactDescriptor {
    #[must_use]
    pub fn is_edge_portable(&self) -> bool {
        matches!(
            self.profile,
            MiddlewareRuntimeProfile::PortableJavaScript | MiddlewareRuntimeProfile::PortableWasm
        )
    }

    #[must_use]
    pub fn is_provider_overlay(&self) -> bool {
        self.provider != MiddlewareProvider::Generic
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MiddlewarePackageManifest {
    pub spec_version: u32,
    pub name: String,
    pub version: String,
    pub semantic_contract_sha256: String,
    #[serde(default)]
    pub phases: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub artifacts: Vec<MiddlewareArtifactDescriptor>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MiddlewarePackageViolation {
    pub code: &'static str,
    pub artifact_id: Option<String>,
    pub message: String,
}

impl MiddlewarePackageManifest {
    #[must_use]
    pub fn validate(&self) -> Vec<MiddlewarePackageViolation> {
        let mut violations = Vec::new();

        if self.spec_version != 1 {
            violations.push(MiddlewarePackageViolation {
                code: "unsupported_spec_version",
                artifact_id: None,
                message: "middleware package spec_version must be 1".to_owned(),
            });
        }

        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            violations.push(MiddlewarePackageViolation {
                code: "missing_package_identity",
                artifact_id: None,
                message: "middleware package name and version are required".to_owned(),
            });
        }

        if !is_sha256_hex(&self.semantic_contract_sha256) {
            violations.push(MiddlewarePackageViolation {
                code: "invalid_semantic_contract_digest",
                artifact_id: None,
                message: "semantic_contract_sha256 must be 64 lowercase hexadecimal characters"
                    .to_owned(),
            });
        }

        if self.artifacts.is_empty() {
            violations.push(MiddlewarePackageViolation {
                code: "missing_artifacts",
                artifact_id: None,
                message: "middleware package must publish at least one artifact".to_owned(),
            });
        }

        let mut ids = BTreeSet::new();
        for artifact in &self.artifacts {
            if !ids.insert(artifact.id.as_str()) {
                violations.push(MiddlewarePackageViolation {
                    code: "duplicate_artifact_id",
                    artifact_id: Some(artifact.id.clone()),
                    message: "artifact ids must be unique within a middleware package".to_owned(),
                });
            }

            if artifact.semantic_contract_sha256 != self.semantic_contract_sha256 {
                violations.push(MiddlewarePackageViolation {
                    code: "semantic_contract_digest_mismatch",
                    artifact_id: Some(artifact.id.clone()),
                    message: "every artifact must identify the package semantic contract digest"
                        .to_owned(),
                });
            }

            if artifact.path.trim().is_empty() || artifact.language.trim().is_empty() {
                violations.push(MiddlewarePackageViolation {
                    code: "invalid_artifact_identity",
                    artifact_id: Some(artifact.id.clone()),
                    message: "artifact language and path are required".to_owned(),
                });
            }

            let profile_kind_matches = match artifact.profile {
                MiddlewareRuntimeProfile::PortableJavaScript => {
                    artifact.kind == MiddlewareArtifactKind::JavaScriptModule
                }
                MiddlewareRuntimeProfile::PortableWasm => {
                    artifact.kind == MiddlewareArtifactKind::WebAssemblyModule
                }
                MiddlewareRuntimeProfile::Native => matches!(
                    artifact.kind,
                    MiddlewareArtifactKind::NativeLibrary | MiddlewareArtifactKind::BeamModule
                ),
                MiddlewareRuntimeProfile::IsolatedProcess => {
                    artifact.kind == MiddlewareArtifactKind::ProcessRpc
                }
            };

            if !profile_kind_matches {
                violations.push(MiddlewarePackageViolation {
                    code: "artifact_profile_kind_mismatch",
                    artifact_id: Some(artifact.id.clone()),
                    message: "artifact kind is incompatible with its runtime profile".to_owned(),
                });
            }
        }

        violations
    }

    #[must_use]
    pub fn select_artifact(
        &self,
        provider: MiddlewareProvider,
        supported_profiles: &[MiddlewareRuntimeProfile],
    ) -> Option<&MiddlewareArtifactDescriptor> {
        if !self.validate().is_empty() {
            return None;
        }

        let provider_specific = self.artifacts.iter().find(|artifact| {
            artifact.provider == provider && supported_profiles.contains(&artifact.profile)
        });
        if provider_specific.is_some() {
            return provider_specific;
        }

        self.artifacts.iter().find(|artifact| {
            artifact.provider == MiddlewareProvider::Generic
                && supported_profiles.contains(&artifact.profile)
        })
    }
}

fn is_sha256_hex(value: &str) -> bool {
    if value.len() != 64 {
        return false;
    }

    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn artifact(
        id: &str,
        language: &str,
        kind: MiddlewareArtifactKind,
        profile: MiddlewareRuntimeProfile,
        provider: MiddlewareProvider,
    ) -> MiddlewareArtifactDescriptor {
        MiddlewareArtifactDescriptor {
            id: id.to_owned(),
            language: language.to_owned(),
            kind,
            profile,
            provider,
            path: format!("dist/{id}"),
            semantic_contract_sha256: DIGEST.to_owned(),
        };
    }

    fn manifest() -> MiddlewarePackageManifest {
        MiddlewarePackageManifest {
            spec_version: 1,
            name: "example-auth".to_owned(),
            version: "1.2.3".to_owned(),
            semantic_contract_sha256: DIGEST.to_owned(),
            phases: vec!["pre_route".to_owned(), "pre_handler".to_owned()],
            capabilities: vec!["headers".to_owned(), "crypto".to_owned()],
            artifacts: vec![
                artifact(
                    "portable-js",
                    "typescript",
                    MiddlewareArtifactKind::JavaScriptModule,
                    MiddlewareRuntimeProfile::PortableJavaScript,
                    MiddlewareProvider::Generic,
                ),
                artifact(
                    "portable-wasm",
                    "rust",
                    MiddlewareArtifactKind::WebAssemblyModule,
                    MiddlewareRuntimeProfile::PortableWasm,
                    MiddlewareProvider::Generic,
                ),
                artifact(
                    "cloudflare-native",
                    "typescript",
                    MiddlewareArtifactKind::JavaScriptModule,
                    MiddlewareRuntimeProfile::PortableJavaScript,
                    MiddlewareProvider::Cloudflare,
                ),
                artifact(
                    "jvm-sidecar",
                    "java",
                    MiddlewareArtifactKind::ProcessRpc,
                    MiddlewareRuntimeProfile::IsolatedProcess,
                    MiddlewareProvider::Generic,
                ),
            ],
        };
    }

    #[test]
    fn provider_overlay_wins_without_becoming_contract_authority() {
        let manifest = manifest();
        let selected = manifest
            .select_artifact(
                MiddlewareProvider::Cloudflare,
                &[MiddlewareRuntimeProfile::PortableJavaScript],
            )
            .expect("cloudflare artifact");

        assert_eq!(selected.id, "cloudflare-native");
        assert_eq!(
            selected.semantic_contract_sha256,
            manifest.semantic_contract_sha256
        );
    }

    #[test]
    fn provider_falls_back_to_portable_artifact() {
        let manifest = manifest();
        let selected = manifest
            .select_artifact(
                MiddlewareProvider::Caddy,
                &[MiddlewareRuntimeProfile::PortableWasm],
            )
            .expect("portable wasm artifact");

        assert_eq!(selected.id, "portable-wasm");
        assert_eq!(selected.provider, MiddlewareProvider::Generic);
    }

    #[test]
    fn java_process_adapter_is_valid_without_wasm() {
        let manifest = manifest();
        let selected = manifest
            .select_artifact(
                MiddlewareProvider::Generic,
                &[MiddlewareRuntimeProfile::IsolatedProcess],
            )
            .expect("jvm sidecar artifact");

        assert_eq!(selected.language, "java");
        assert_eq!(selected.kind, MiddlewareArtifactKind::ProcessRpc);
    }

    #[test]
    fn digest_mismatch_fails_closed() {
        let mut manifest = manifest();
        manifest.artifacts[0].semantic_contract_sha256 = "f".repeat(64);

        assert!(
            manifest
                .validate()
                .iter()
                .any(|violation| violation.code == "semantic_contract_digest_mismatch")
        );
        assert!(
            manifest
                .select_artifact(
                    MiddlewareProvider::Cloudflare,
                    &[MiddlewareRuntimeProfile::PortableJavaScript]
                )
                .is_none()
        );
    }
}
