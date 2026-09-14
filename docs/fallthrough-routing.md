# Final route fall-through contract

`ores-middleware` provides one shared response for the **outermost server/router ownership boundary**: the point reached only after every application route has declined the request target.

This boundary is intentionally different from normal resource and method errors:

- **404 Not Found** remains correct inside a matched route when the requested resource does not exist, and is available as a compatibility status for the final boundary.
- **405 Method Not Allowed** remains correct when the router knows the target route but that route does not support the request method. The router should emit its normal `Allow` header. Do not pass a known-route method mismatch into this fall-through handler.
- **421 Misdirected Request** is the ORES default only for the final ownership boundary: this server has no route that claims the target URI. RFC 9110 also says a proxy MUST NOT generate 421, so install this in the origin/application server, not in a generic forward proxy.

## Stable wire response

Default GET-style response:

```http
HTTP/1.1 421 Misdirected Request
Content-Type: application/problem+json; charset=utf-8
Cache-Control: no-store
X-Content-Type-Options: nosniff
Content-Length: ...

{"type":"urn:ores:error:route-unmatched","title":"No route matched","status":421,"code":"ores.route.unmatched","detail":"The request target is not handled by this server."}
```

The response deliberately does not echo the path, query, method, route inventory, framework name, or upstream topology. `HEAD` returns the same status and headers, including the would-be GET `Content-Length`, with an empty body.

For deployments that must preserve conventional framework behavior at the outer boundary, select 404 compatibility. The problem `code` remains `ores.route.unmatched`, so telemetry can distinguish final router fall-through from a resource-level 404.

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

`FallthroughResponse` implements Axum `IntoResponse` when the `axum` feature is enabled. Use `FallthroughConfig::not_found_compatibility()` for 404 compatibility.

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

Use `{ status: 404 }` only for explicit compatibility.

## Go

```go
mux := http.NewServeMux()
// register application routes on mux

// When using a router that supports an explicit NotFound/final-handler hook,
// install this handler in that hook rather than replacing per-route 404s.
final := oresmiddleware.DefaultFinalFallthroughHandler()
_ = final
```

For 404 compatibility:

```go
final := oresmiddleware.FinalFallthroughHandler(
    oresmiddleware.FallthroughOptions{NotFoundCompatibility: true},
)
```

The exact router wiring differs between `net/http`, chi, Gin, Echo, Fiber, and other adapters; the shared handler owns only the response semantics.

## Gleam

The Gleam package returns a framework-neutral value so Mist, Wisp, or another server adapter can translate it without adding an HTTP-framework dependency to `ores_middleware`:

```gleam
import ores_middleware/fallthrough

let response = fallthrough.default_final_fallthrough("GET")
```

Adapters copy `status`, `headers`, and `body` into their framework response. `NotFoundCompatibility` selects 404.

## Rollout rule

Install this only as the **last unmatched-route handler** in each server. Keep resource-level 404 logic and router-native 405 logic unchanged. Record `ores.route.unmatched` separately in request metrics so a rise in fall-through traffic is visible without exposing requested paths in the response body.
