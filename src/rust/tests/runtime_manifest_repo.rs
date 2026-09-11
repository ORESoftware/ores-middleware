#![forbid(unsafe_code)]

use ores_middleware::{RuntimeManifestError, admit_server_stack};

const REPOSITORY_MANIFEST: &str = include_str!("../../../.ores-mw.toml");

#[test]
fn repository_manifest_admits_the_actual_default_server_stack() {
    assert_eq!(
        admit_server_stack(
            REPOSITORY_MANIFEST,
            None,
            "contracts/fixtures/stack.minimal.json",
        ),
        Ok(())
    );
}

#[test]
fn repository_manifest_rejects_runtime_stack_path_drift() {
    assert_eq!(
        admit_server_stack(REPOSITORY_MANIFEST, None, "contracts/fixtures/other.json"),
        Err(RuntimeManifestError::StackConfigMismatch)
    );
}

#[test]
fn repository_manifest_rejects_client_selection_for_server_installation() {
    let client = REPOSITORY_MANIFEST
        .replace(
            "repository_mode = \"server-only\"",
            "repository_mode = \"hybrid\"",
        )
        .replace(
            "default_target = \"portable-adapters\"",
            "default_target = \"browser\"",
        )
        + r#"

[[targets]]
name = "browser"
role = "client"
roots = ["src"]
middleware = "propagation-only"
propagate_headers = ["traceparent"]
"#;
    assert_eq!(
        admit_server_stack(&client, None, "contracts/fixtures/stack.minimal.json",),
        Err(RuntimeManifestError::ClientTarget)
    );
}
