function fail(message) {
  throw new Error(message);
}

if (typeof Blob === "undefined") fail("Blob is unavailable");
const blob = new Blob(["ores", "-", "middleware"], { type: "text/plain" });
if (blob.type !== "text/plain") fail(`Blob type mismatch: ${blob.type}`);
if ((await blob.text()) !== "ores-middleware") fail("Blob text mismatch");
if (blob.size !== new TextEncoder().encode("ores-middleware").byteLength) fail("Blob size mismatch");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "blob",
  status: "passed",
  size: blob.size,
}));
