import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { CONTRACT_DIGEST_HEADER, decideDocs } from "../src/index.js";

const fixturePath = fileURLToPath(
  new URL("../../../fixtures/docs-serving-conformance.tsv", import.meta.url),
);

function optional(value) {
  return value === "-" ? undefined : value;
}

async function cases() {
  const text = await readFile(fixturePath, "utf8");
  return text
    .split(/\r?\n/)
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => {
      const fields = line.split("\t");
      assert.equal(fields.length, 11, `invalid fixture row: ${line}`);
      const [
        name,
        method,
        path,
        accept,
        format,
        runtimeContractDigest,
        docsContractDigest,
        action,
        status,
        representation,
        headOnly,
      ] = fields;
      return {
        name,
        request: {
          method,
          path,
          accept: optional(accept),
          format: optional(format),
          runtimeContractDigest: optional(runtimeContractDigest),
          docsContractDigest: optional(docsContractDigest),
        },
        expected: {
          action,
          status: status === "-" ? undefined : Number(status),
          representation: optional(representation),
          headOnly: headOnly === "true",
        },
      };
    });
}

for (const item of await cases()) {
  test(item.name, () => {
    const decision = decideDocs(item.request);
    assert.equal(decision.action, item.expected.action);
    assert.equal(decision.status, item.expected.status);
    assert.equal(decision.representation, item.expected.representation);
    assert.equal(decision.headOnly, item.expected.headOnly);

    if (decision.action === "pass") {
      assert.deepEqual(decision.headers, {});
    } else {
      assert.equal(decision.headers["Cache-Control"], "no-store");
      assert.equal(decision.headers["X-Content-Type-Options"], "nosniff");
      assert.match(decision.headers.Vary, /X-Ores-Docs-Format/);
    }
    if (decision.action === "method-not-allowed") {
      assert.equal(decision.headers.Allow, "GET, HEAD");
    }
    if (decision.representation === "html") {
      assert.equal(decision.headers["X-Frame-Options"], "DENY");
      assert.match(decision.headers["Content-Security-Policy"], /frame-ancestors 'none'/);
    }
    if (item.request.docsContractDigest?.length === 64 && decision.action === "serve") {
      assert.equal(decision.headers[CONTRACT_DIGEST_HEADER], item.request.docsContractDigest);
    }
    const serialized = JSON.stringify(decision);
    assert.ok(!serialized.includes("Authorization"));
    assert.ok(!serialized.includes("Bearer"));
  });
}
