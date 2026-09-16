import gleam/list

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

/// Compose middleware in consumer declaration order. The first middleware is
/// outermost, so it runs first on the request path and last on the response
/// path. ores_middleware deliberately does not decide which stages exist.
pub fn compose(
  handler: Handler(request, response),
  middleware: List(Middleware(request, response)),
) -> Handler(request, response) {
  list.fold_right(middleware, handler, fn(next, stage) {
    wrap(stage, next)
  })
}
