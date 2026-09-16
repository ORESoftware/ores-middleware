/// A generic consumer-owned provider port.
///
/// `input`, `output`, and `error` stay application-defined, so an auth SDK,
/// database client, RPC client, or any other concrete implementation can be
/// captured by the supplied function without becoming an ores_middleware
/// dependency.
pub type Provider(input, output, error) {
  Provider(verify: fn(input) -> Result(output, error))
}

pub fn provider(
  verify: fn(input) -> Result(output, error),
) -> Provider(input, output, error) {
  Provider(verify:)
}

pub fn verify(
  provider: Provider(input, output, error),
  input: input,
) -> Result(output, error) {
  let Provider(verify:) = provider
  verify(input)
}

/// Carries independently typed request and consumer context values without
/// coupling the generic provider to HTTP, OTP, an auth SDK, or ORES context.
pub type ContextualInput(request, context) {
  ContextualInput(request: request, context: context)
}

pub fn contextual_provider(
  verify: fn(request, context) -> Result(output, error),
) -> Provider(ContextualInput(request, context), output, error) {
  provider(fn(input) { verify(input.request, input.context) })
}

/// Generic request/response handler. The response type can itself represent an
/// async/effectful computation, keeping this abstraction runtime-agnostic.
pub type Handler(request, response) {
  Handler(run: fn(request) -> response)
}

pub fn handler(run: fn(request) -> response) -> Handler(request, response) {
  Handler(run:)
}

pub fn run(handler: Handler(request, response), request: request) -> response {
  let Handler(run:) = handler
  run(request)
}

/// Middleware transforms one generic handler into another. No stage names,
/// framework types, or ordering constraints are built into the abstraction.
pub type Middleware(request, response) {
  Middleware(wrap: fn(Handler(request, response)) -> Handler(request, response))
}

pub fn middleware(
  wrap: fn(Handler(request, response)) -> Handler(request, response),
) -> Middleware(request, response) {
  Middleware(wrap:)
}

pub fn wrap(
  middleware: Middleware(request, response),
  next: Handler(request, response),
) -> Handler(request, response) {
  let Middleware(wrap:) = middleware
  wrap(next)
}

/// Consumer-owned metadata for middleware stages. ORES preserves the name but
/// assigns it no semantics and never uses it to reorder the stack.
pub type NamedMiddleware(request, response) {
  NamedMiddleware(name: String, middleware: Middleware(request, response))
}

/// Compose middleware in consumer declaration order. The first middleware is
/// outermost, so it runs first on the request path and last on the response
/// path. ores_middleware deliberately does not decide which stages exist.
pub fn compose(
  handler: Handler(request, response),
  middleware: List(Middleware(request, response)),
) -> Handler(request, response) {
  case middleware {
    [] -> handler
    [stage, ..rest] -> wrap(stage, compose(handler, rest))
  }
}

pub fn compose_named(
  handler: Handler(request, response),
  stages: List(NamedMiddleware(request, response)),
) -> Handler(request, response) {
  case stages {
    [] -> handler
    [NamedMiddleware(middleware: stage, ..), ..rest] ->
      wrap(stage, compose_named(handler, rest))
  }
}
