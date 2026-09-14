import assert from "node:assert/strict";
import { test } from "node:test";
import { gzipSync } from "node:zlib";

import {
  PayloadDecodeError,
  canonicalHeaderMap,
  decodeRequestPayload,
  normalizeContentType
} from "../dist/payload-codecs.js";

const limits = { maxCompressedBytes: 1024, maxDecodedBytes: 4096, timeoutMs: 1000 };

test("normalizes aliases and canonical lowercase headers", () => {
  assert.equal(normalizeContentType("application/x-msgpack; charset=binary"), "application/msgpack");
  assert.equal(normalizeContentType("application/x-protobuf"), "application/protobuf");
  const map = canonicalHeaderMap(new Headers({ "X-ORES-Request-ID": "r1", "Content-Type": "application/json" }));
  assert.equal(map["x-ores-request-id"], "r1");
  assert.equal(map["content-type"], "application/json");
});

test("decodes bounded JSON and exposes jsonPayload map", async () => {
  const request = new Request("https://example.test/items", {
    method: "POST",
    headers: { "content-type": "application/json", "X-ORES-Request-ID": "r1" },
    body: JSON.stringify({ hello: "world", count: 2 })
  });
  const decoded = await decodeRequestPayload(request, limits);
  assert.equal(decoded.representation, "application/json");
  assert.deepEqual(decoded.jsonPayload, { hello: "world", count: 2 });
  assert.equal(decoded.headers["x-ores-request-id"], "r1");
});

test("decompresses gzip before JSON parsing and rewrites transport headers", async () => {
  const body = gzipSync(Buffer.from(JSON.stringify({ compressed: true })));
  const request = new Request("https://example.test/items", {
    method: "POST",
    headers: { "content-type": "application/json", "content-encoding": "gzip" },
    body
  });
  const decoded = await decodeRequestPayload(request, limits);
  assert.deepEqual(decoded.jsonPayload, { compressed: true });
  assert.equal(decoded.request.headers.get("content-encoding"), null);
  assert.equal(Number(decoded.request.headers.get("content-length")), decoded.bytes.byteLength);
});

test("dispatches MessagePack decoder without treating bytes as JSON", async () => {
  let seen = false;
  const request = new Request("https://example.test/items", {
    method: "POST",
    headers: { "content-type": "application/msgpack" },
    body: new Uint8Array([0x81, 0xa1, 0x61, 0x01])
  });
  const decoded = await decodeRequestPayload(request, {
    ...limits,
    decoders: {
      messagePack: ({ bytes, mediaType }) => {
        seen = true;
        assert.equal(mediaType, "application/msgpack");
        assert.deepEqual([...bytes], [0x81, 0xa1, 0x61, 0x01]);
        return { a: 1 };
      }
    }
  });
  assert.equal(seen, true);
  assert.deepEqual(decoded.value, { a: 1 });
});

test("protobuf decoding requires both message descriptor and decoder", async () => {
  const request = () => new Request("https://example.test/items", {
    method: "POST",
    headers: { "content-type": "application/protobuf" },
    body: new Uint8Array([0x08, 0x01])
  });
  await assert.rejects(
    () => decodeRequestPayload(request(), limits),
    (error) => error instanceof PayloadDecodeError && error.code === "protobuf_message_type_required" && error.status === 422
  );
  const decoded = await decodeRequestPayload(request(), {
    ...limits,
    protobufMessageType: "ores.example.Widget",
    decoders: {
      protobuf: ({ protobufMessageType, bytes }) => ({ protobufMessageType, bytes: [...bytes] })
    }
  });
  assert.deepEqual(decoded.value, { protobufMessageType: "ores.example.Widget", bytes: [0x08, 0x01] });
});

test("rejects XML DTD/entity declarations before custom parser dispatch", async () => {
  const request = new Request("https://example.test/xml", {
    method: "POST",
    headers: { "content-type": "application/xml" },
    body: "<!DOCTYPE foo [<!ENTITY xxe SYSTEM 'file:///etc/passwd'>]><foo>&xxe;</foo>"
  });
  await assert.rejects(
    () => decodeRequestPayload(request, { ...limits, decoders: { xml: () => ({}) } }),
    (error) => error instanceof PayloadDecodeError && error.code === "unsafe_xml_document" && error.status === 400
  );
});

test("unsupported content encoding fails with 415", async () => {
  const request = new Request("https://example.test/items", {
    method: "POST",
    headers: { "content-type": "application/json", "content-encoding": "compress" },
    body: "{}"
  });
  await assert.rejects(
    () => decodeRequestPayload(request, limits),
    (error) => error instanceof PayloadDecodeError && error.status === 415
  );
});
