function fail(message) {
  throw new Error(message);
}

if (!globalThis.Bun) {
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "bun-fetch-primitives",
    status: "skipped",
    reason: "not-bun",
  }));
} else {
  const request = new Request("https://example.test/path?x=1", {
    method: "POST",
    headers: { "x-test": "yes", "content-type": "application/json" },
    body: JSON.stringify({ value: "ok" }),
  });
  if (request.method !== "POST") fail("Bun Request method mismatch");
  if (request.headers.get("x-test") !== "yes") fail("Bun Headers mismatch");
  if ((await request.clone().json()).value !== "ok") fail("Bun Request clone/body mismatch");

  const response = Response.json({ ok: true }, { status: 201, headers: { "x-test": "yes" } });
  if (response.status !== 201) fail("Bun Response status mismatch");
  if (response.headers.get("x-test") !== "yes") fail("Bun Response headers mismatch");
  if ((await response.json()).ok !== true) fail("Bun Response body mismatch");

  const controller = new AbortController();
  controller.abort("expected");
  if (!controller.signal.aborted) fail("Bun AbortSignal mismatch");

  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "bun-fetch-primitives",
    status: "passed",
  }));
}
