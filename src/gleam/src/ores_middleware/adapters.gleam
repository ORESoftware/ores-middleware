import ores_middleware.{type Middleware}

/// Framework-neutral entry point used by gleam_http servers.
pub fn gleam_http(middleware: Middleware, request, next) {
  middleware(request, next)
}

/// Adapter boundary for Mist request handlers.
pub fn mist(middleware: Middleware, request, next) {
  middleware(request, next)
}

/// Adapter boundary for Wisp applications.
pub fn wisp(middleware: Middleware, request, next) {
  middleware(request, next)
}

/// Adapter boundary for Cowboy handlers running on OTP.
pub fn cowboy(middleware: Middleware, request, next) {
  middleware(request, next)
}

/// Adapter boundary for custom OTP request processes.
pub fn otp(middleware: Middleware, request, next) {
  middleware(request, next)
}
