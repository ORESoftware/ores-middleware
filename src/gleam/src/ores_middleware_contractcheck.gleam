import gleam/io
import ores_middleware

pub fn main() {
  io.println(ores_middleware.descriptor_json())
}
