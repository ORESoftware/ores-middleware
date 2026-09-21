use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config_discovery::{self, Origin},
    runtime_manifest::{
        admit_server_stack, ManifestLoadError, MANIFEST_ENV_PREFIX, MANIFEST_FILE_NAME,
    },
};

/// Stable schema identifier for middleware runtime-config admission evidence.
pub const MANIFEST_ADMISSION_EVIDENCE_SCHEMA: &str =
    "ores.middleware.runtime-config-admission/v1";

/// Evidence that the owner library discovered and admitted the exact middleware
/// manifest bytes before the caller proceeds to runtime construction.
///
/// This receipt deliberately records no configuration values. The SHA-256 binds
/// the receipt to the exact UTF-8 document while `path` records which file won
/// discovery. Consumers can persist or log the receipt without copying secrets
/// or policy values out of the owner library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestAdmissionEvidence {
    pub schema: String,
    pub path: PathBuf,
    pub source_sha256: String,
    pub source_bytes: usize,
    pub at_repo_root: bool,
    pub target_name: Option<String>,
    pub expected_stack_config: String,
}

impl ManifestAdmissionEvidence {
    #[must_use]
    pub fn valid_digest(&self) -> bool {
        self.source_sha256.len() == 64
            && self
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

/// Discover and admit `.ores-mw.toml` from the process environment, returning
/// path-and-digest evidence for the exact admitted bytes.
///
/// # Errors
/// Returns the same discovery/admission errors as the compatibility
/// `admit_server_stack_from_env` entry point.
pub fn admit_server_stack_with_evidence_from_env(
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<ManifestAdmissionEvidence, ManifestLoadError> {
    let origin = config_discovery::origin_from_env(MANIFEST_ENV_PREFIX);
    admit_server_stack_with_evidence_from(&origin, target_name, expected_stack_config)
}

/// Discover and admit `.ores-mw.toml` from an explicit origin, returning
/// evidence for the exact selected document.
///
/// # Errors
/// Returns [`ManifestLoadError::Discovery`] when discovery fails and
/// [`ManifestLoadError::Manifest`] when the selected document does not admit
/// the requested server stack.
pub fn admit_server_stack_with_evidence_from(
    origin: &Origin,
    target_name: Option<&str>,
    expected_stack_config: &str,
) -> Result<ManifestAdmissionEvidence, ManifestLoadError> {
    let found = config_discovery::discover(MANIFEST_FILE_NAME, origin)
        .map_err(ManifestLoadError::Discovery)?;
    admit_server_stack(&found.source, target_name, expected_stack_config)
        .map_err(ManifestLoadError::Manifest)?;

    let digest = Sha256::digest(found.source.as_bytes());
    Ok(ManifestAdmissionEvidence {
        schema: MANIFEST_ADMISSION_EVIDENCE_SCHEMA.to_owned(),
        path: found.path,
        source_sha256: format!("{digest:x}"),
        source_bytes: found.source.len(),
        at_repo_root: found.at_repo_root,
        target_name: target_name.map(str::to_owned),
        expected_stack_config: expected_stack_config.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const GOOD: &str = r#"schema_version = 1
repository_mode = "server-only"
default_target = "api"

[[targets]]
name = "api"
role = "server"
roots = ["src"]
middleware = "stack"
stack_config = "config/middleware.json"
"#;

    fn fixture_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ores-middleware-admission-evidence-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).expect("git marker");
        root
    }

    #[test]
    fn evidence_binds_selected_path_and_exact_source_bytes() {
        let root = fixture_root("success");
        let path = root.join(MANIFEST_FILE_NAME);
        fs::write(&path, GOOD).expect("manifest");
        let origin = Origin {
            explicit_dir: Some(root.clone()),
            ..Origin::default()
        };

        let evidence = admit_server_stack_with_evidence_from(
            &origin,
            None,
            "config/middleware.json",
        )
        .expect("admitted evidence");

        assert_eq!(evidence.schema, MANIFEST_ADMISSION_EVIDENCE_SCHEMA);
        // Discovery canonicalises its start, and on macOS the temp dir is /var, a
        // symlink to /private/var: compare canonical paths or this passes on
        // Linux CI and fails on every developer Mac.
        assert_eq!(
            std::fs::canonicalize(&evidence.path).expect("canonical evidence path"),
            std::fs::canonicalize(&path).expect("canonical expected path")
        );
        assert_eq!(evidence.source_bytes, GOOD.len());
        assert!(evidence.at_repo_root);
        assert!(evidence.valid_digest());
        assert_eq!(
            evidence.source_sha256,
            format!("{:x}", Sha256::digest(GOOD.as_bytes()))
        );
        assert_eq!(evidence.target_name, None);
        assert_eq!(evidence.expected_stack_config, "config/middleware.json");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_admission_never_returns_evidence() {
        let root = fixture_root("failure");
        fs::write(root.join(MANIFEST_FILE_NAME), GOOD).expect("manifest");
        let origin = Origin {
            explicit_dir: Some(root.clone()),
            ..Origin::default()
        };

        let error = admit_server_stack_with_evidence_from(
            &origin,
            None,
            "config/not-the-admitted-stack.json",
        )
        .expect_err("mismatched stack config must fail");
        assert_eq!(error.code(), "runtime_stack_config_mismatch");
        let _ = fs::remove_dir_all(root);
    }
}
