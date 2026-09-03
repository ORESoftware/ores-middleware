import gleam/list
import gleeunit
import ores_middleware

pub fn main() { gleeunit.main() }

pub fn descriptor_has_standard_surface_test() {
  let descriptor = ores_middleware.descriptor()
  assert list.length(descriptor.capabilities) == 23
  assert list.length(descriptor.operation_symbols |> gleam/dict.to_list) == 7
}

pub fn production_rejects_test_only_middleware_test() {
  let config = ores_middleware.default_config("test")
  let config = ores_middleware.Config(
    ..config,
    environment: ores_middleware.Production,
    fault_injection_enabled: True,
    test_auth_bypass_enabled: True,
  )
  assert list.length(ores_middleware.validate_config(config)) >= 2
}
