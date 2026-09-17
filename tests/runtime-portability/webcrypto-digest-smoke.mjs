function fail(message) {
  throw new Error(message);
}

if (!globalThis.crypto?.subtle) fail("WebCrypto subtle API is unavailable");
const bytes = new TextEncoder().encode("ores-middleware");
const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", bytes));
const hex = Array.from(digest, (value) => value.toString(16).padStart(2, "0")).join("");
if (hex.length !== 64) fail(`SHA-256 digest length mismatch: ${hex.length}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "webcrypto-digest",
  status: "passed",
  sha256: hex,
}));
