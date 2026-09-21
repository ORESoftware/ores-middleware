/// Capabilities currently enforced directly by the provider-neutral non-HTTP
/// Lambda callback boundary.
///
/// This list is intentionally narrower than the HTTP middleware capability set.
/// In particular, it does not claim HTTP headers/TLS/CORS/compression, and it
/// does not claim callback auth/rate-limit/idempotency until those stages have
/// callback-native inputs instead of synthetic HTTP metadata.
pub const LAMBDA_INVOCATION_CAPABILITIES: &[&str] = &[
    "request-context",
    "panic-recovery",
    "request-id",
    "trace-context",
    "structured-logging",
    "deadline-timeout",
    "payload-limit",
];

#[must_use]
pub const fn lambda_invocation_capabilities() -> &'static [&'static str] {
    LAMBDA_INVOCATION_CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_boundary_does_not_overclaim_http_or_unimplemented_policy() {
        let capabilities = lambda_invocation_capabilities();
        for unsupported in [
            "headers",
            "tls-policy",
            "compression",
            "security-headers",
            "auth",
            "rate-limit",
            "idempotency",
            "ip-policy",
        ] {
            assert!(!capabilities.contains(&unsupported), "{unsupported}");
        }
    }
}
