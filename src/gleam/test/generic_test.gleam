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

  assert generic.verify(provider, "v7:alice") == Ok("alice")
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

  assert generic.run(composed, "request")
    == "request-id>consumer-auth-v7>tenant-rate-limit>request:handler<tenant-rate-limit<consumer-auth-v7<request-id"
}
