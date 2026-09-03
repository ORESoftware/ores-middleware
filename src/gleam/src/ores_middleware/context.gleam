import ores_middleware.{type RequestContext, current_context}

/// Direct field access when a typed request context is already available.
pub fn request_id(context: RequestContext) -> String {
  context.request_id
}

pub fn trace_id(context: RequestContext) -> String {
  context.trace_id
}

pub fn user_id(context: RequestContext) -> String {
  context.user_id
}

pub fn logged_in_user_id(context: RequestContext) -> String {
  user_id(context)
}

pub fn tenant_id(context: RequestContext) -> String {
  context.tenant_id
}

fn current_value(read: fn(RequestContext) -> String) -> Result(String, Nil) {
  case current_context() {
    Ok(context) -> Ok(read(context))
    Error(reason) -> Error(reason)
  }
}

/// Reads only the request ID from the current BEAM process scope.
pub fn current_request_id() -> Result(String, Nil) {
  current_value(request_id)
}

pub fn current_trace_id() -> Result(String, Nil) {
  current_value(trace_id)
}

pub fn current_user_id() -> Result(String, Nil) {
  current_value(user_id)
}

pub fn current_logged_in_user_id() -> Result(String, Nil) {
  current_user_id()
}

pub fn current_tenant_id() -> Result(String, Nil) {
  current_value(tenant_id)
}
