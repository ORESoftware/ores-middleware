#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "rollout-rust-server.py"
REVISION = "0123456789abcdef0123456789abcdef01234567"


def fixture(source: str, *extra: str) -> tuple[Path, str]:
    temporary = tempfile.TemporaryDirectory()
    root = Path(temporary.name)
    (root / "src").mkdir()
    (root / "Cargo.toml").write_text(
        """[package]
name = "fixture-server"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
"""
    )
    (root / "src/main.rs").write_text(source)
    command = [
        "python3",
        str(SCRIPT),
        "--source",
        "src/main.rs",
        "--manifest",
        "Cargo.toml",
        "--revision",
        REVISION,
        *extra,
    ]
    subprocess.run(command, cwd=root, check=True, capture_output=True, text=True)
    output = (root / "src/main.rs").read_text()
    manifest = (root / "Cargo.toml").read_text()
    assert f'rev = "{REVISION}"' in manifest
    assert "features = [\"axum\"]" in manifest
    assert (root / "docs/ores-middleware.md").is_file()
    assert "/target/" in (root / ".gitignore").read_text()
    # Keep the TemporaryDirectory alive long enough for all assertions.
    fixture._temporary = temporary
    return root, output


def direct_router() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app).await?;
    Ok(())
}
"""
    )
    assert "frameworks::axum_audit::install_from_env" in output
    assert "app," in output
    assert ")?" in output


def configured_router_is_explicit() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app).await?;
    Ok(())
}
""",
        "--rate-limit-mode",
        "configured",
    )
    assert "frameworks::axum::install_from_env" in output
    assert "frameworks::axum_audit" not in output


def make_service_receiver() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}
"""
    )
    assert ")?.into_make_service_with_connect_info::<SocketAddr>()" in output


def precomputed_service() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let service = router.into_make_service();
    axum::serve(listener, service).await?;
    Ok(())
}
""",
        "--router-var",
        "router",
    )
    install = output.index("let router = ores_middleware")
    conversion = output.index("let service = router.into_make_service")
    assert install < conversion


def assigned_serve_router_shadow_preserves_statement_boundary() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let public = axum::serve(
        listener,
        router.into_make_service(),
    );
    public.await?;
    Ok(())
}
""",
        "--router-var",
        "router",
    )
    assert "    let router = ores_middleware" in output
    assert "    let public = axum::serve" in output
    assert "let public = let router" not in output
    assert output.index("let router = ores_middleware") < output.index(
        "let public = axum::serve"
    )


def associated_function() -> None:
    _root, output = fixture(
        """async fn run() -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, axum::ServiceExt::<Request>::into_make_service(app)).await?;
    Ok(())
}
"""
    )
    assert "ServiceExt::<Request>::into_make_service" in output
    assert "install_from_env" in output
    assert "app," in output


def infallible_entrypoint() -> None:
    _root, output = fixture(
        """async fn main() {
    axum::serve(listener, app).await.expect("serve");
}
""",
        "--error-mode",
        "expect",
    )
    assert '.expect("valid ORES middleware configuration")' in output


def main() -> None:
    direct_router()
    configured_router_is_explicit()
    make_service_receiver()
    precomputed_service()
    assigned_serve_router_shadow_preserves_statement_boundary()
    associated_function()
    infallible_entrypoint()
    print("Rust rollout helper fixtures passed")


if __name__ == "__main__":
    main()
