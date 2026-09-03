use std::sync::Arc;

use axum::Router;

use crate::MiddlewareStack;

/// Installs the canonical stack for MASH servers (Maud + Axum + server-rendered HTML + HTMX).
pub fn install(router: Router, stack: Arc<MiddlewareStack>) -> Router {
    super::axum::install(router, stack)
}
