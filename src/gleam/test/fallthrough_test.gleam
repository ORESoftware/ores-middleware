import gleam/string
import gleeunit
import ores_middleware/fallthrough

pub fn main() {
  gleeunit.main()
}

pub fn default_fallthrough_is_421_test() {
  let response = fallthrough.default_final_fallthrough("GET")
  assert response.status == 421
  assert response.content_length > 0
  assert string.contains(response.body, fallthrough.unmatched_route_error_code)
  assert string.contains(response.body, "/private/secret") == False
}

pub fn not_found_compatibility_is_explicit_test() {
  let response =
    fallthrough.final_fallthrough(
      "GET",
      fallthrough.NotFoundCompatibility,
    )
  assert response.status == 404
  assert string.contains(response.body, "\"status\":404")
  assert string.contains(response.body, fallthrough.unmatched_route_error_code)
}

pub fn head_omits_body_but_preserves_length_test() {
  let get_response = fallthrough.default_final_fallthrough("GET")
  let head_response = fallthrough.default_final_fallthrough("HEAD")
  assert head_response.status == get_response.status
  assert head_response.body == ""
  assert head_response.content_length == get_response.content_length
  assert head_response.content_length > 0
}
