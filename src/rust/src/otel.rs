use std::{collections::BTreeMap, future::Future};

use crate::RequestContext;

// Re-export the canonical Rust logger so downstream services can use
// ores-middleware as the single integration surface.
pub use next_loggers::*;

/// Maps the portable, serializable middleware context into ores-otel's native
/// task context. Only allow-listed correlation metadata is copied.
pub fn to_ores_log_context(context: &RequestContext) -> LogContext {
    let mut fields = JsonObject::from_iter([
        (
            "request.id".into(),
            Value::String(context.request_id.clone()),
        ),
        (
            "trace.id".into(),
            Value::String(context.trace_id.clone()),
        ),
        (
            "request.started_at_unix_ms".into(),
            Value::from(context.started_at_unix_ms),
        ),
    ]);
    if let Some(user_id) = &context.user_id {
        fields.insert("user.id".into(), Value::String(user_id.clone()));
    }
    if let Some(tenant_id) = &context.tenant_id {
        fields.insert("tenant.id".into(), Value::String(tenant_id.clone()));
    }
    if let Some(locale) = &context.locale {
        fields.insert("request.locale".into(), Value::String(locale.clone()));
    }
    if let Some(deadline) = context.deadline_unix_ms {
        fields.insert(
            "request.deadline_unix_ms".into(),
            Value::from(deadline),
        );
    }

    let logged_in_user = context
        .user_id
        .as_ref()
        .map(|user_id| {
            JsonObject::from_iter([("id".into(), Value::String(user_id.clone()))])
        })
        .unwrap_or_default();
    let baggage = context
        .baggage
        .iter()
        .filter(|(key, _)| key.starts_with("otel."))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let trace_ids = (!context.trace_id.is_empty())
        .then(|| vec![context.trace_id.clone()])
        .unwrap_or_default();

    LogContext {
        logged_in_user,
        fields,
        trace_id: (!context.trace_id.is_empty()).then(|| context.trace_id.clone()),
        trace_ids,
        span_id: context.span_id.clone(),
        baggage,
        routine_id: Some(context.request_id.clone()),
        tags: vec!["ores-middleware".into(), "request".into()],
        ..LogContext::default()
    }
    .normalized()
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

/// Runs a future with ores-otel's poll-safe task context. File/module loggers
/// imported elsewhere can call `info_context`, `warn_context`, and friends and
/// still receive this request's correlation fields.
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
            assert_eq!(baggage.get("otel.vendor"), Some(&Value::String("allowed".into())));
            assert!(!baggage.contains_key("authorization"));
        }
        assert_eq!(current_log_context(), LogContext::default());
    }
}
