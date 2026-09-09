import assert from "node:assert/strict";
import test from "node:test";
import { boundRequestBody, PayloadTooLargeError } from "../dist/request-body.js";
const encoder = new TextEncoder();
function streamed(chunks, headers = {}, options = {}) {
  let index = 0;
  return new Request("http://localhost/upload", {
    method: "POST", duplex: "half", headers, signal: options.signal,
    body: new ReadableStream({
      pull(controller) {
        if (index < chunks.length) controller.enqueue(chunks[index++]);
        else if (!options.stall) controller.close();
      },
      cancel: options.cancel
    })
  });
}
for (const headers of [{}, { "content-length": "1" }, { "transfer-encoding": "chunked" }]) {
  test(`actual bytes bound with headers ${JSON.stringify(headers)}`, async () => {
    let cancelled = false;
    const request = streamed([encoder.encode("1234"), encoder.encode("5678")], headers, { stall: true, cancel() { cancelled = true; } });
    await assert.rejects(boundRequestBody(request, 7, 100), PayloadTooLargeError);
    assert.equal(cancelled, true);
  });
}
test("exact bound is accepted and downstream body remains readable repeatedly via clones", async () => {
  const bounded = await boundRequestBody(streamed([encoder.encode("12"), encoder.encode("34")]), 4, 100);
  assert.equal(bounded.headers.get("content-length"), "4");
  assert.equal(await bounded.clone().text(), "1234"); assert.equal(await bounded.text(), "1234");
});
test("byte bounds count UTF-8 bytes, not characters", async () => {
  await assert.rejects(boundRequestBody(streamed([encoder.encode("éé")]), 3, 100), PayloadTooLargeError);
});
test("body deadline cancels a stalled producer without waiting for its cancel promise", async () => {
  let cancelled = false;
  const request = streamed([], {}, { stall: true, cancel() { cancelled = true; return new Promise(() => {}); } });
  await assert.rejects(boundRequestBody(request, 8, 10), { name: "TimeoutError" });
  assert.equal(cancelled, true);
});
test("abort while reading cancels the body", async () => {
  const controller = new AbortController(); let cancelled = false;
  const request = streamed([], {}, { stall: true, signal: controller.signal, cancel() { cancelled = true; } });
  const result = boundRequestBody(request, 8, 100);
  controller.abort(new DOMException("cancelled", "AbortError"));
  await assert.rejects(result, { name: "AbortError" }); assert.equal(cancelled, true);
});
test("pre-aborted requests cannot be admitted", async () => {
  const controller = new AbortController(); controller.abort();
  await assert.rejects(boundRequestBody(new Request("https://example.test", { signal: controller.signal }), 8, 100), { name: "AbortError" });
});
test("empty requests retain identity and invalid limits fail closed", async () => {
  const request = new Request("https://example.test");
  assert.equal(await boundRequestBody(request, 8, 100), request);
  for (const limit of [0, -1, 1.5, Infinity, NaN]) {
    await assert.rejects(boundRequestBody(request, limit, 100), TypeError);
  }
});
test("request child logger is preserved without copying arbitrary properties", async () => {
  const request = new Request("https://example.test", { method: "POST", body: "x" });
  request.log = {}; request.untrustedIdentity = "admin";
  const result = await boundRequestBody(request, 8, 100);
  assert.equal(result.log, request.log); assert.equal(result.untrustedIdentity, undefined);
});
test("non-byte stream chunks are rejected", async () => {
  await assert.rejects(boundRequestBody(streamed(["not bytes"]), 32, 100), TypeError);
});
