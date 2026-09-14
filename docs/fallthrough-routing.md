# Final route fall-through contract

`ores-middleware` provides one shared response for the **final unmatched-route boundary**: the point reached only after every application route has declined the request target.

There is no standards-defined status code that is more specific than 404 for "the request reached the intended origin, but no route/resource matches this target." The stronger distinction therefore lives in the problem type and machine-readable error code, not in a non-standard or misleading HTTP status.

The boundary rules are:

- **404 Not Found** is the default for the final unmatched-route handler. The stable `ores.route.unmatched` code distinguishes router fall-through from a resource-level 404 in telemetry and clients.
- **405 Method Not Allowed** remains correct when the router knows the target route but that route does not support the request method. The router should emit its normal `Allow` header. Do not pass a known-route method mismatch into this fall-through handler.
- **421 Misdirected Request** is available only as an explicit authority-mismatch mode. RFC 9110 uses 421 when the target URI does not match an origin for which the server is configured, or does not match the connection context. Clients may retry a 421 on another connection, and a proxy must not generate it, so it is not the generic no-route response.

## Stable wire response

Default GET-style response:

```http
HTTP/1.1 404 Not Found
Content-Type: application/problem+json; charset=utf-8
Cache-Control: no-store
X-Content-Type-Options: nosniff
Content-Length: ...

{"type":"urn:ores:error:route-unmatched","title":"No route matched","status":404,"code":"ores.route.unmatched","detail":"The request target is not handled by this server."}
```

The response deliberately does not echo the path, query, method, route inventory, framework name, or upstream topology. `HEAD` returns the same status and headers, including the would-be GET `Content-Length`, with an empty body.

For a true origin/connection mismatch, select the explicit 421 mode. The problem `code` remains `ores.route.unmatched`, while the HTTP status communicates that the current authority/connection is not appropriate.

## Rust / Axum

```rust
use axum::{Router, http::Method};
use ores_middleware::fallthrough::{
    FallthroughConfig,
    FallthroughResponse,
    final_fallthrough_response,
};

async fn final_route(method: Method) -> FallthroughResponse {
    final_fallthrough_response(&method, FallthroughConfig::default())
}

let app = Router::<()>::new()
    // .route(...)
    .fallback(final_route);
```

`FallthroughResponse` implements Axum `IntoResponse` when the `axum` feature is enabled. Use `FallthroughConfig::misdirected_authority()` only at an authority/connection mismatch boundary.

## Node.js / TypeScript

Fetch/WHATWG-style servers:

```ts
import { createFinalFallthroughResponse } from "@oresoftware/ores-middleware/fallthrough";

return createFinalFallthroughResponse(request);
```

Native Node and Connect/Express-style final handler:

```ts
import { nodeFinalFallthroughHandler } from "@oresoftware/ores-middleware/fallthrough";

app.use(nodeFinalFallthroughHandler());
```

For a true authority mismatch only:

```ts
return createFinalFallthroughResponse(request, {
  statusMode: "misdirected-authority"
});
```

## Go

```go
mux := http.NewServeMux()
// register application routes on mux

// When using a router that supports an explicit NotFound/final-handler hook,
// install this handler in that hook rather than replacing per-route 404s.
final := oresmiddleware.DefaultFinalFallthroughHandler()
_ = final
```

For an authority mismatch only:

```go
final := oresmiddleware.FinalFallthroughHandler(
    oresmiddleware.FallthroughOptions{MisdirectedAuthority: true},
)
```

The exact router wiring differs between `net/http`, chi, Gin, Echo, Fiber, and other adapters; the shared handler owns only the response semantics.

## Gleam

The Gleam package returns a framework-neutral value so Mist, Wisp, or another server adapter can translate it without adding an HTTP-framework dependency to `ores_middleware`:

```gleam
import ores_middleware/fallthrough

let response = fallthrough.default_final_fallthrough("GET")
```

Adapters copy `status`, `headers`, and `body` into their framework response. `MisdirectedAuthority` selects 421 only for an origin/connection mismatch.

## Rollout rule

Install the default handler only as the **last unmatched-route handler** in each server. Keep resource-level 404 logic and router-native 405 logic unchanged. Record `ores.route.unmatched` separately in request metrics so a rise in fall-through traffic is visible without exposing requested paths in the response body.

If a server also owns virtual-host or connection-authority dispatch, it may use the explicit 421 mode at that earlier boundary, before normal application routing. Do not use that mode from a generic forward proxy.
