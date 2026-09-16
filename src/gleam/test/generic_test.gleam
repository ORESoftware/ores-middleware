import gleeunit
import ores_middleware/generic

pub fn main() {
  gleeunit.main()
}

pub fn generic_provider_keeps_sdk_consumer_owned_test() {
  let version = "v7"
  let provider =
    generic.provider(fn(token) {
      case token == version <> ":alice" {
        True -> Ok("alice")
        False -> Error("rejected")
      }
    })

  let result = generic.verify(provider, "v7:alice")
  assert result == Ok("alice")
}

pub fn contextual_provider_keeps_request_context_and_failure_generic_test() {
  let provider =
    generic.contextual_provider(fn(request, context) {
      case request == "ok" {
        True -> Ok(context <> ":accepted")
        False -> Error(42)
      }
    })

  let accepted =
    generic.verify(
      provider,
      generic.ContextualInput(request: "ok", context: "tenant-a"),
    )
  let rejected =
    generic.verify(
      provider,
      generic.ContextualInput(request: "bad", context: "tenant-a"),
    )

  assert accepted == Ok("tenant-a:accepted")
  assert rejected == Error(42)
}

pub fn generic_middleware_order_is_consumer_owned_test() {
  let stage = fn(name) {
    generic.middleware(fn(next) {
      generic.handler(fn(request) {
        name <> ">" <> generic.run(next, request) <> "<" <> name
      })
    })
  }

  let base = generic.handler(fn(request) { request <> ":handler" })
  let composed =
    generic.compose(base, [
      stage("request-id"),
      stage("consumer-auth-v7"),
      stage("tenant-rate-limit"),
    ])
  let result = generic.run(composed, "request")
  let expected =
    "request-id>consumer-auth-v7>tenant-rate-limit>request:handler<tenant-rate-limit<consumer-auth-v7<request-id"

  assert result == expected
}

pub fn named_middleware_names_are_not_interpreted_test() {
  let stage = fn(name) {
    generic.NamedMiddleware(
      name: name,
      middleware: generic.middleware(fn(next) {
        generic.handler(fn(request) { name <> ">" <> generic.run(next, request) })
      }),
    )
  }
  let base = generic.handler(fn(request) { request })
  let composed =
    generic.compose_named(base, [
      stage("custom-z"),
      stage("auth-provider-v42"),
      stage("custom-a"),
    ])
  let result = generic.run(composed, "request")
  let expected = "custom-z>auth-provider-v42>custom-a>request"

  assert result == expected
}
