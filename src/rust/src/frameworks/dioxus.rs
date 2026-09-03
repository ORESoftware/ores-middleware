use std::sync::Arc;

use axum::Router;

use crate::MiddlewareStack;

/// Installs the canonical stack around a Dioxus fullstack/Axum router.
pub fn install(router: Router, stack: Arc<MiddlewareStack>) -> Router {
    super::axum::install(router, stack)
}
