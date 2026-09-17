function fail(message) {
  throw new Error(message);
}

if (!globalThis.Deno) {
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "deno-fetch-primitives",
    status: "skipped",
    reason: "not-deno",
  }));
} else {
  for (const name of ["Request", "Response", "Headers", "AbortController", "crypto", "fetch"]) {
    if (typeof globalThis[name] === "undefined") fail(`Deno global ${name} is unavailable`);
  }
  const request = new Request("https://example.test/path", { headers: { "x-test": "yes" } });
  if (request.headers.get("x-test") !== "yes") fail("Deno Headers mismatch");
  const id = crypto.randomUUID();
  if (!/^[0-9a-f-]{36}$/u.test(id)) fail("Deno crypto.randomUUID mismatch");
  console.log(JSON.stringify({
    schema: "ores.middleware.js-runtime-portability-smoke/v1",
    suite: "deno-fetch-primitives",
    status: "passed",
  }));
}
