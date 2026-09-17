import gleeunit
import ores_middleware/generic

pub fn main() {
  gleeunit.main()
}

pub fn contextual_middleware_preserves_consumer_order_test() {
  let stage = fn(name) {
    generic.contextual_middleware(fn(next) {
      generic.contextual_handler(fn(request, context) {
        name
        <> ":"
        <> context
        <> ">"
        <> generic.run_contextual(next, request, context)
        <> "<"
        <> name
      })
    })
  }
  let base =
    generic.contextual_handler(fn(request, context) {
      context <> ":" <> request
    })
  let composed =
    generic.compose_contextual(base, [stage("auth"), stage("tenant")])

  assert generic.run_contextual(composed, "request", "t-1")
    == "auth:t-1>tenant:t-1>t-1:request<tenant<auth"
}

pub fn named_contextual_middleware_keeps_opaque_duplicate_names_test() {
  let stage = fn(name) {
    generic.NamedContextualMiddleware(
      name: name,
      middleware: generic.contextual_middleware(fn(next) {
        generic.contextual_handler(fn(request, context) {
          name <> ":" <> context <> ">" <> generic.run_contextual(
            next,
            request,
            context,
          )
        })
      }),
    )
  }
  let base = generic.contextual_handler(fn(request, _context) { request })
  let composed =
    generic.compose_named_contextual(base, [
      stage("auth"),
      stage("auth"),
      stage("custom-z"),
    ])

  assert generic.run_contextual(composed, "request", "tenant-a")
    == "auth:tenant-a>auth:tenant-a>custom-z:tenant-a>request"
}

pub fn empty_contextual_chains_are_identity_transforms_test() {
  let base =
    generic.contextual_handler(fn(request, context) {
      context <> ":" <> request
    })
  let unnamed = generic.compose_contextual(base, [])
  let named = generic.compose_named_contextual(base, [])

  assert generic.run_contextual(unnamed, "request", "t-1") == "t-1:request"
  assert generic.run_contextual(named, "request", "t-1") == "t-1:request"
}
