use std::future::Future;

use crate::RequestContext;

// Re-export the canonical Rust logger so downstream services can use
// ores-middleware as the single integration surface.
pub use next_loggers::*;

/// Maps the portable middleware snapshot into the canonical ores-otel request
/// context. The middleware conversion is the trust boundary and filters
/// baggage before this logger projection is created.
pub fn to_ores_log_context(context: &RequestContext) -> LogContext {
    context.to_canonical().to_log_context()
}

/// A request-specific logger handle suitable for Axum request extensions.
/// Unlike cloning the root logger alone, every event is permanently decorated
/// with the immutable request/user/tenant snapshot captured at construction.
#[derive(Clone)]
pub struct RequestLogger {
    logger: Logger,
    context: LogContext,
}

impl RequestLogger {
    pub fn new(logger: Logger, context: &RequestContext) -> Self {
        Self {
            logger,
            context: to_ores_log_context(context),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn context(&self) -> &LogContext {
        &self.context
    }

    pub fn trace(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.trace(values), &self.context)
    }

    pub fn debug(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.debug(values), &self.context)
    }

    pub fn info(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.info(values), &self.context)
    }

    pub fn warn(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.warn(values), &self.context)
    }

    pub fn error(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.error(values), &self.context)
    }

    pub fn fatal(&self, values: Vec<Value>) -> Event {
        apply_log_context(self.logger.fatal(values), &self.context)
    }
}

/// Compatibility wrapper over the same poll-safe carrier used by
/// `context::run_with_context`. New middleware code should enter one of these
/// APIs once, not nest both around the same request.
pub fn run_with_ores_log_context<F: Future>(
    context: &RequestContext,
    future: F,
) -> ContextFuture<F> {
    with_log_context_async(to_ores_log_context(context), future)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;

    fn request_context() -> RequestContext {
        RequestContext {
            request_id: "request-42".into(),
            trace_id: "0123456789abcdef0123456789abcdef".into(),
            span_id: Some("0123456789abcdef".into()),
            tenant_id: Some("tenant-7".into()),
            user_id: Some("user-42".into()),
            locale: Some("en-US".into()),
            started_at_unix_ms: 1,
            deadline_unix_ms: Some(2),
            baggage: BTreeMap::from([
                ("otel.vendor".into(), "allowed".into()),
                ("authorization".into(), "must-not-propagate".into()),
            ]),
        }
    }

    #[tokio::test]
    async fn request_and_file_loggers_share_poll_safe_context() {
        let transport = Arc::new(MemoryTransport::default());
        let logger = Logger::new(Options {
            app_name: "middleware-test".into(),
            name: Some("orders".into()),
            transports: vec![transport.clone()],
            console: false,
            ..Options::default()
        });
        let context = request_context();
        let request_logger = RequestLogger::new(logger.clone(), &context);

        request_logger
            .warn(vec![Value::String("slow dependency".into())])
            .send()
            .expect("request log");
        run_with_ores_log_context(&context, async {
            logger
                .info_context(vec![Value::String("handler reached".into())])
                .send()
                .expect("file log");
        })
        .await;

        let records = transport.records();
        assert_eq!(records.len(), 2);
        for record in records {
            assert_eq!(record.fields["request.id"], "request-42");
            assert_eq!(record.fields["tenant.id"], "tenant-7");
            assert_eq!(
                record.logged_in_user.as_ref().and_then(|user| user.get("id")),
                Some(&Value::String("user-42".into()))
            );
            let baggage = record.fields["otel.baggage"]
                .as_object()
                .expect("otel baggage object");
            assert_eq!(
                baggage.get("otel.vendor"),
                Some(&Value::String("allowed".into()))
            );
            assert!(!baggage.contains_key("authorization"));
        }
        assert_eq!(current_log_context(), LogContext::default());
    }
}
