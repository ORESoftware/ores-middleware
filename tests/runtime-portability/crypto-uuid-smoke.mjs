function fail(message) {
  throw new Error(message);
}

if (!globalThis.crypto || typeof globalThis.crypto.randomUUID !== "function") {
  fail("crypto.randomUUID is unavailable");
}
const values = new Set(Array.from({ length: 64 }, () => globalThis.crypto.randomUUID()));
if (values.size !== 64) fail("crypto.randomUUID produced duplicate values in smoke sample");
for (const value of values) {
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(value)) {
    fail(`invalid UUID v4 syntax: ${value}`);
  }
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "crypto-uuid",
  status: "passed",
  samples: values.size,
}));
