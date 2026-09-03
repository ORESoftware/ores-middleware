# Request-scoped logging with ores-otel

`ores-middleware` is the framework-facing integration layer. It imports and re-exposes the language SDKs from [`ores-otel/ores.otel.log`](https://github.com/ores-otel/ores.otel.log); the logger repository does not depend on HTTP frameworks.

```text
ores-middleware -> ores-otel/ores.otel.log
```

The portable `RequestContext` remains a serializable, data-only contract. A runtime-native logger handle lives beside it in `Request`, `context.Context`, Axum extensions, a BEAM handler argument, `Plug.Conn.assigns`, or an Erlang request map.

## Correlation contract

The integration creates a request logger only after authentication has resolved and decorates events with:

- `request.id` and `trace.id`;
- span correlation when the runtime exposes it;
- authenticated `user.id` and `tenant.id`;
- request locale, start time, and deadline;
- authenticated baggage whose key starts with `otel.`.

Do not place authorization headers, cookies, raw tokens, credentials, request bodies, email addresses, or unrestricted baggage in request context or log fields. Logger delivery is fail-open for the request path, and this integration does not install or replace a global OpenTelemetry provider.

## TypeScript, Next.js, Node.js, Bun, and Deno

The adapter installs the canonical ores-otel `AsyncLocalStorage` provider once, derives a child logger after authentication, and exposes it as `request.log`. A `WeakMap` fallback is retained for runtimes with sealed `Request` objects. Separately imported file loggers read the same ALS frame.

```ts
import {
  createMiddleware,
  defaultConfig,
  type AuthVerifier,
} from "@oresoftware/ores-middleware";
import {
  createLogger,
  createOresOtelMiddleware,
} from "@oresoftware/ores-middleware/otel";

const rootLog = createLogger({ appName: "orders-api", name: "server" });
const fileLog = createLogger({ appName: "orders-api", name: "orders-route" });
const config = defaultConfig("orders-api");

export const middleware = createOresOtelMiddleware(config, {
  logger: rootLog,
  authVerifier,
});

export async function handle(request: Request): Promise<Response> {
  return middleware(request, async (scopedRequest) => {
    await scopedRequest.log.info("request-local event").send();
    await fileLog.info("file logger inherits request context").send();
    return Response.json({ ok: true });
  });
}
```

Use the framework adapter at the actual server/router boundary. Do not create a second ALS store in individual routes.

## Go

Go keeps propagation explicit in `context.Context`. `WrapWithOresLogger` installs both the portable context and an ores-otel child. Handlers may use the compact `Log(ctx)` facade, fetch the concrete child, or pass the same context to a file-level logger.

```go
rootLog := oresmiddleware.NewOresLogger(oresmiddleware.OresLoggerOptions{
    AppName: "orders-api",
    Name:    "server",
})
fileLog := oresmiddleware.NewOresLogger(oresmiddleware.OresLoggerOptions{
    AppName: "orders-api",
    Name:    "orders-route",
})

handler := stack.WrapWithOresLogger(rootLog, http.HandlerFunc(
    func(w http.ResponseWriter, r *http.Request) {
        _ = oresmiddleware.Log(r.Context()).Info("request-local event")

        if requestLog, ok := oresmiddleware.OresLoggerFromContext(r.Context()); ok {
            _ = requestLog.WarnContext(r.Context(), "concrete child logger").Send()
        }

        _ = fileLog.InfoContext(r.Context(), "file logger inherits request context").Send()
        w.WriteHeader(http.StatusNoContent)
    },
))
```

Any goroutine must receive the derived context explicitly. Never emulate goroutine-local storage.

## Rust, Axum, Mash, Leptos, and Dioxus

Rust uses the logger SDK's poll-safe task context. The Axum installer places `RequestLogger` in request extensions and scopes the entire handler future, so separately owned module loggers can use `info_context`, `warn_context`, and related methods.

```rust
use std::sync::Arc;
use axum::{Extension, Router};
use ores_middleware::{MiddlewareStack, RequestLogger};
use ores_middleware::otel::{Logger, Options, Value};

let root_log = Logger::new(Options {
    app_name: "orders-api".into(),
    name: Some("server".into()),
    ..Options::default()
});

let app: Router = ores_middleware::frameworks::axum::install_with_ores_logger(
    app,
    Arc::new(stack),
    root_log,
);

async fn route(Extension(log): Extension<RequestLogger>) {
    let _ = log
        .info(vec![Value::String("request-local event".into())])
        .send();
}
```

The same installer backs Mash, Leptos, and Dioxus-on-Axum. Do not use thread-local request state: Tokio futures may move between worker threads.

## Gleam

Gleam passes a typed `RequestLogger` to the handler and also installs process-local logger context while that callback runs.

```gleam
import gleam/json
import ores_middleware/otel

auto_result = otel.create_middleware(config, hooks, root_logger)

case auto_result {
  Error(issues) -> handle_configuration_error(issues)
  Ok(middleware) ->
    middleware(request, fn(scoped_request, request_log) {
      let _ =
        otel.info(request_log, "request-local event", [json.string("orders")])
        |> otel.send

      next(scoped_request)
    })
}
```

Pass context explicitly when spawning another BEAM process; process-local state does not cross process boundaries automatically.

## Elixir, Plug, and Phoenix

Install `OresMiddleware.OTelPlug` immediately after `OresMiddleware.Plug`. The integration exposes the pinned logger as both `conn.assigns.log` and `conn.assigns.ores_log`, and restores the exact prior process-local context in the before-send cleanup path.

```elixir
plug OresMiddleware.Plug, stack: {MyApp.Middleware, :stack, []}
plug OresMiddleware.OTelPlug, logger: {MyApp.Log, :root, []}

# In a controller or downstream plug:
def show(conn, _params) do
  {:ok, _record} = conn.assigns.log.info.("request-local event")
  {:ok, _record} =
    OresMiddleware.OTel.warn(conn.assigns.log, "slow dependency", %{
      "dependency.name" => "inventory"
    })

  send_resp(conn, 204, "")
end
```

An ordinary logger created once in a module can call `ORESoftware.NextLoggers.info/3` while the request process is scoped and receive the same correlation fields.

## Erlang, Cowboy, and OTP

The framework-neutral adapter passes `Request#{log := RequestLogger, ores_log := RequestLogger}` to the handler. The logger map includes `info`, `warn`, and `error` closures plus field-aware variants.

```erlang
{ok, Middleware} = ores_middleware_otel:create_middleware(Config, Hooks, RootLogger),
Response = Middleware(Request, fun(RequestWithLog) ->
    RequestLog = maps:get(log, RequestWithLog),
    Info = maps:get(info, RequestLog),
    {ok, _Event} = Info(<<"request-local event">>),
    #{status => 204, headers => #{}, body => <<>>}
end).
```

For Cowboy, place the root logger in middleware environment as `ores_otel_logger`. The adapter stores the pinned logger in request metadata and handler environment under `log` and `ores_log`.

## Required service tests

Each adopting server should prove:

1. request, trace, authenticated user, and tenant identifiers appear on both request-local and file-level logger events;
2. two concurrent requests cannot observe each other's context;
3. success, error, panic/crash, cancellation, and timeout paths restore or discard context;
4. non-`otel.*` baggage and credential-bearing inputs never enter logs;
5. logger transport failure does not replace the HTTP response or hide the original handler failure;
6. background work either receives an explicit context snapshot or intentionally starts a new correlation scope.
