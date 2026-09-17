function fail(message) {
  throw new Error(message);
}

const source = "ORES ✓ middleware 🚀";
const encoded = new TextEncoder().encode(source);
const decoded = new TextDecoder("utf-8", { fatal: true }).decode(encoded);
if (decoded !== source) fail(`UTF-8 round trip mismatch: ${decoded}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "text-encoding",
  status: "passed",
  bytes: encoded.byteLength,
}));
