from pathlib import Path


def replace_once(path: str, before: str, after: str) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    count = text.count(before)
    if count != 1:
        raise RuntimeError(f"expected exactly one match in {path}, found {count}: {before[:120]!r}")
    target.write_text(text.replace(before, after, 1), encoding="utf-8")


replace_once(
    "src/ts/src/index.ts",
    '''import { currentContext, runWithContext } from "./context.js";\n\nexport { currentContext, runWithContext };''',
    '''import { currentContext, runWithContext } from "./context.js";\nimport {\n  operationContextFromRequestContext,\n  runOperationBoundary,\n  type OperationFailure,\n  type OperationFailureReporter\n} from "./operation.js";\n\nexport { currentContext, runWithContext };''',
)

replace_once(
    "src/ts/src/index.ts",
    '''  telemetry?: {\n    started(context: RequestContext, request: Request): Promise<void> | void;\n    finished(context: RequestContext, request: Request, response: Response, durationMs: number): Promise<void> | void;\n  };\n  syncObserver?:''',
    '''  telemetry?: {\n    started(context: RequestContext, request: Request): Promise<void> | void;\n    finished(context: RequestContext, request: Request, response: Response, durationMs: number): Promise<void> | void;\n  };\n  /** Optional audited sink; defaults to the bounded ores-otel reporter. */\n  operationFailureReporter?: OperationFailureReporter;\n  syncObserver?:''',
)

replace_once(
    "src/ts/src/index.ts",
    '''  return async (request, next) => {\n    const started = now();\n    const contentLength = Number(request.headers.get("content-length") ?? "0");''',
    '''  return async (request, next) => {\n    const started = now();\n    const requestId = validToken(request.headers.get(config.settings.requestIdHeader)) ?? crypto.randomUUID();\n    const traceId = parseTraceId(request.headers.get(config.settings.traceHeader)) ?? crypto.randomUUID().replaceAll("-", "");\n    let context: RequestContext = {\n      requestId,\n      traceId,\n      locale: request.headers.get("accept-language") ?? undefined,\n      startedAtUnixMs: started,\n      deadlineUnixMs: started + config.settings.timeoutMs,\n      baggage: {}\n    };\n\n    const preAuthOutcome = await runOperationBoundary(\n      { transport: "http", scope: "request", name: "middleware.pre_auth", signal: request.signal },\n      async () => {\n    const contentLength = Number(request.headers.get("content-length") ?? "0");''',
)

replace_once(
    "src/ts/src/index.ts",
    '''    const requestId = validToken(request.headers.get(config.settings.requestIdHeader)) ?? crypto.randomUUID();\n    const traceId = parseTraceId(request.headers.get(config.settings.traceHeader)) ?? crypto.randomUUID().replaceAll("-", "");\n    const context: RequestContext = {\n      requestId,\n      traceId,\n      locale: request.headers.get("accept-language") ?? undefined,\n      startedAtUnixMs: started,\n      deadlineUnixMs: started + config.settings.timeoutMs,\n      baggage: {}\n    };\n\n''',
    '''''',
)

replace_once(
    "src/ts/src/index.ts",
    '''    context.userId = auth.userId;\n    context.tenantId = auth.tenantId;\n    for (const [key, value] of Object.entries(auth.claims ?? {})) if (key.startsWith("otel.")) context.baggage[key] = value;\n    if (config.integrations.sharedAuth.mode !== "disabled" && !context.userId) return problem(401, "authentication_required", "shared-auth did not establish a user");''',
    '''    context = {\n      ...context,\n      userId: auth.userId,\n      tenantId: auth.tenantId,\n      baggage: {\n        ...context.baggage,\n        ...Object.fromEntries(\n          Object.entries(auth.claims ?? {}).filter(([key]) => key.startsWith("otel."))\n        )\n      }\n    };\n\n    const authenticatedOutcome = await runOperationBoundary(\n      { transport: "http", scope: "request", name: "middleware.request", signal: request.signal },\n      async () => {\n    if (config.integrations.sharedAuth.mode !== "disabled" && !context.userId) return problem(401, "authentication_required", "shared-auth did not establish a user");''',
)

replace_once(
    "src/ts/src/index.ts",
    '''    await dependencies.telemetry?.started(context, request);\n    let response: Response;\n    try {\n      response = await runWithContext(context, () =>\n        withDeadline(config.settings.timeoutMs, () => next(request))\n      );\n    } catch (error) {\n      response = error instanceof DeadlineError\n        ? problem(504, "deadline_exceeded", "request deadline exceeded")\n        : problem(500, "internal_error", "request handler failed");\n    }''',
    '''    await dependencies.telemetry?.started(context, request);\n    let response = await withDeadline(config.settings.timeoutMs, () => next(request));''',
)

replace_once(
    "src/ts/src/index.ts",
    '''    if (idempotencyKey && response.status >= 200 && response.status < 300) {\n      const body = new Uint8Array(await response.clone().arrayBuffer());\n      await idempotencyStore.set(idempotencyKey, { status: response.status, headers: [...response.headers.entries()], body, expiresAt: now() + config.settings.idempotency.ttlSeconds * 1_000 });\n    }\n    return response;\n  };\n}\n''',
    '''    if (idempotencyKey && response.status >= 200 && response.status < 300) {\n      const body = new Uint8Array(await response.clone().arrayBuffer());\n      await idempotencyStore.set(idempotencyKey, { status: response.status, headers: [...response.headers.entries()], body, expiresAt: now() + config.settings.idempotency.ttlSeconds * 1_000 });\n    }\n    return response;\n      },\n      {\n        context: operationContextFromRequestContext(context),\n        reportFailure: dependencies.operationFailureReporter\n      }\n    );\n    return authenticatedOutcome.ok\n      ? authenticatedOutcome.value\n      : operationFailureResponse(config, context, authenticatedOutcome.failure);\n      },\n      {\n        context: operationContextFromRequestContext(context),\n        reportFailure: dependencies.operationFailureReporter\n      }\n    );\n    return preAuthOutcome.ok\n      ? preAuthOutcome.value\n      : operationFailureResponse(config, context, preAuthOutcome.failure);\n  };\n}\n''',
)

replace_once(
    "src/ts/src/index.ts",
    '''class DeadlineError extends Error {}''',
    '''class DeadlineError extends Error {\n  constructor() {\n    super("request deadline exceeded");\n    this.name = "TimeoutError";\n  }\n}''',
)

replace_once(
    "src/ts/src/index.ts",
    '''function problem(status: number, code: string, detail: string): Response {''',
    '''function operationFailureResponse(\n  config: MiddlewareConfig,\n  context: RequestContext,\n  failure: OperationFailure\n): Response {\n  const response = failure.kind === "deadline_exceeded"\n    ? problem(504, "deadline_exceeded", "request deadline exceeded")\n    : failure.kind === "cancelled"\n      ? problem(499, "request_cancelled", "request was cancelled")\n      : problem(500, "internal_error", "request processing failed");\n  return attachHeaders(config, context, response);\n}\n\nfunction problem(status: number, code: string, detail: string): Response {''',
)

replace_once(
    "src/ts/src/operation.ts",
    '''/** Captures both the middleware carrier and ores-otel's native log carrier. */\nexport function captureOperationContext(): CapturedOperationContext {\n  return {\n    requestContext: captureContext(),\n    // An empty child frame is intentional: it prevents a callback captured\n    // outside a request from inheriting whichever request invokes it later.\n    logContext: captureLogContext() ?? {}\n  };\n}\n''',
    '''/** Captures both the middleware carrier and ores-otel's native log carrier. */\nexport function captureOperationContext(): CapturedOperationContext {\n  return {\n    requestContext: captureContext(),\n    // An empty child frame is intentional: it prevents a callback captured\n    // outside a request from inheriting whichever request invokes it later.\n    logContext: captureLogContext() ?? {}\n  };\n}\n\n/**\n * Builds a fresh immutable operation carrier from an explicit request context.\n * This is used by framework adapters before policy hooks run and again after\n * authentication enriches the actor/tenant fields. Only allow-listed values\n * become ambient log fields.\n */\nexport function operationContextFromRequestContext(\n  context: RequestContext\n): CapturedOperationContext {\n  const fields: Record<string, unknown> = {\n    "request.id": context.requestId,\n    "trace.id": context.traceId,\n    "request.started_at_unix_ms": context.startedAtUnixMs\n  };\n  if (context.spanId) fields["span.id"] = context.spanId;\n  if (context.userId) fields["user.id"] = context.userId;\n  if (context.tenantId) fields["tenant.id"] = context.tenantId;\n  if (context.locale) fields["request.locale"] = context.locale;\n  if (context.deadlineUnixMs !== undefined) {\n    fields["request.deadline_unix_ms"] = context.deadlineUnixMs;\n  }\n  for (const [key, value] of Object.entries(context.baggage)) {\n    if (key.startsWith("otel.")) fields[`baggage.${key}`] = value;\n  }\n  return { requestContext: context, logContext: { fields } };\n}\n''',
)

Path("src/ts/test/http-lifecycle-boundary.test.mjs").write_text(
    '''import test from "node:test";\nimport assert from "node:assert/strict";\n\nimport {\n  createMiddleware,\n  currentContext,\n  defaultConfig\n} from "../dist/index.js";\nimport { getLogContext } from "@oresoftware/next-loggers/context";\n\nfunction testConfig() {\n  const config = defaultConfig("http-lifecycle-boundary-test");\n  config.settings.tls.mode = "disabled";\n  config.settings.tls.requireHttps = false;\n  config.settings.rateLimit.enabled = false;\n  config.settings.compression.enabled = false;\n  config.settings.idempotency.enabled = false;\n  return config;\n}\n\nfunction request(requestId) {\n  return new Request("http://example.test/profile", {\n    headers: {\n      accept: "application/json",\n      "x-request-id": requestId,\n      traceparent: `00-${requestId.padEnd(32, "0").slice(0, 32)}-0123456789abcdef-01`\n    }\n  });\n}\n\nfunction recorder(reports) {\n  return async ({ failure }) => {\n    reports.push({\n      failure,\n      requestId: currentContext()?.requestId,\n      userId: currentContext()?.userId,\n      logRequestId: getLogContext()?.fields?.["request.id"],\n      logUserId: getLogContext()?.fields?.["user.id"]\n    });\n  };\n}\n\ntest("authentication exceptions are contained inside the base request scope", async () => {\n  const reports = [];\n  const middleware = createMiddleware(testConfig(), {\n    operationFailureReporter: recorder(reports),\n    authVerifier: async (_request, context) => {\n      assert.equal(currentContext()?.requestId, context.requestId);\n      assert.equal(getLogContext()?.fields?.["request.id"], context.requestId);\n      throw new Error("private authentication detail");\n    }\n  });\n\n  const response = await middleware(request("auth-failure"), async () => {\n    throw new Error("handler must not run");\n  });\n  const body = await response.text();\n\n  assert.equal(response.status, 500);\n  assert.equal(response.headers.get("x-request-id"), "auth-failure");\n  assert.doesNotMatch(body, /private authentication detail/);\n  assert.equal(reports.length, 1);\n  assert.equal(reports[0].requestId, "auth-failure");\n  assert.equal(reports[0].logRequestId, "auth-failure");\n  assert.equal(currentContext(), undefined);\n  assert.equal(getLogContext(), undefined);\n});\n\ntest("post-processing exceptions retain authenticated actor and tenant context", async () => {\n  const reports = [];\n  const middleware = createMiddleware(testConfig(), {\n    operationFailureReporter: recorder(reports),\n    authVerifier: async () => ({\n      userId: "user-42",\n      tenantId: "tenant-7",\n      claims: { "otel.plan": "pro", ignored: "secret" }\n    }),\n    captureSchema: async () => {\n      assert.equal(currentContext()?.userId, "user-42");\n      assert.equal(currentContext()?.tenantId, "tenant-7");\n      assert.equal(getLogContext()?.fields?.["user.id"], "user-42");\n      assert.equal(getLogContext()?.fields?.["tenant.id"], "tenant-7");\n      assert.equal(getLogContext()?.fields?.["baggage.otel.plan"], "pro");\n      assert.equal(getLogContext()?.fields?.["baggage.ignored"], undefined);\n      throw new Error("private schema detail");\n    }\n  });\n\n  const response = await middleware(request("post-failure"), async () => {\n    assert.equal(currentContext()?.userId, "user-42");\n    assert.equal(getLogContext()?.fields?.["user.id"], "user-42");\n    return Response.json({ ok: true });\n  });\n  const body = await response.text();\n\n  assert.equal(response.status, 500);\n  assert.equal(response.headers.get("x-request-id"), "post-failure");\n  assert.doesNotMatch(body, /private schema detail/);\n  assert.equal(reports.length, 1);\n  assert.equal(reports[0].requestId, "post-failure");\n  assert.equal(reports[0].userId, "user-42");\n  assert.equal(reports[0].logUserId, "user-42");\n  assert.equal(currentContext(), undefined);\n  assert.equal(getLogContext(), undefined);\n});\n\ntest("parallel policy failures never bleed request or log context", async () => {\n  const reports = [];\n  const middleware = createMiddleware(testConfig(), {\n    operationFailureReporter: recorder(reports),\n    authVerifier: async (_request, context) => {\n      await new Promise((resolve) => setTimeout(resolve, Number(context.requestId.slice(1)) % 4));\n      assert.equal(currentContext()?.requestId, context.requestId);\n      assert.equal(getLogContext()?.fields?.["request.id"], context.requestId);\n      throw new Error("isolated policy failure");\n    }\n  });\n\n  const responses = await Promise.all(\n    Array.from({ length: 32 }, (_, slot) =>\n      middleware(request(`r${slot}`), async () => Response.json({ ok: true }))\n    )\n  );\n\n  assert.ok(responses.every((response) => response.status === 500));\n  assert.equal(reports.length, 32);\n  for (const report of reports) {\n    assert.equal(report.requestId, report.failure.requestId);\n    assert.equal(report.logRequestId, report.failure.requestId);\n  }\n  assert.equal(new Set(reports.map((report) => report.failure.requestId)).size, 32);\n  assert.equal(currentContext(), undefined);\n  assert.equal(getLogContext(), undefined);\n});\n''',
    encoding="utf-8",
)
