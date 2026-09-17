function fail(message) {
  throw new Error(message);
}

const url = new URL("https://example.test/a%20b?x=1&x=2#fragment");
if (url.pathname !== "/a%20b") fail(`pathname mismatch: ${url.pathname}`);
if (url.searchParams.getAll("x").join(",") !== "1,2") fail("duplicate query parameter semantics drifted");
url.searchParams.append("space", "a b");
if (!url.toString().includes("space=a+b") && !url.toString().includes("space=a%20b")) {
  fail(`query encoding mismatch: ${url.toString()}`);
}

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "url-semantics",
  status: "passed",
}));
