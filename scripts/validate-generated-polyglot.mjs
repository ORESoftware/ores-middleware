#!/usr/bin/env node
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(process.argv[2] ?? process.cwd());
const output = path.join(root, 'target', 'schema-convergence');
const authorities = ['typespec', 'json-schema-openapi'];
const commonArtifacts = [
  'model.json',
  'sql/schema.sql',
  'typescript/idempotency_record.d.ts',
  'typescript/idempotency_record.mjs',
  'rust/idempotency_record.rs',
  'golang/idempotency_record.go',
  'gleam/idempotency_record.gleam',
  'elixir/idempotency_record.ex',
  'erlang/idempotency_record.erl',
  'diesel/idempotency_record.rs',
  'seaorm/idempotency_record.rs',
];

const read = (authority, relative) =>
  readFile(path.join(output, authority, relative), 'utf8');

const receipt = JSON.parse(
  await readFile(path.join(output, 'receipt.json'), 'utf8'),
);
assert.equal(receipt.schema, 'ores.persistence-polyglot-convergence/v2');
assert.equal(receipt.status, 'passed');
assert.equal(receipt.zeroUnexplainedFindings, true);
assert.deepEqual(receipt.authorities, authorities);
assert.equal(receipt.lanes.length, 2);
for (const lane of receipt.lanes) {
  assert.ok(authorities.includes(lane.authority));
  assert.match(lane.sourceSha256, /^[0-9a-f]{64}$/u);
  assert.ok(lane.artifacts.length >= commonArtifacts.length);
  for (const artifact of lane.artifacts) {
    assert.match(artifact.sha256, /^[0-9a-f]{64}$/u);
    assert.ok(Number.isInteger(artifact.bytes) && artifact.bytes > 0);
  }
}

for (const relative of commonArtifacts) {
  const [typespec, jsonSchema] = await Promise.all(
    authorities.map((authority) => read(authority, relative)),
  );
  assert.equal(
    typespec,
    jsonSchema,
    `independent generated artifact mismatch: ${relative}`,
  );
}

const good = {
  id: 'id-1',
  tenantId: 'tenant-1',
  idempotencyKey: 'key-1',
  requestHash: 'hash-1',
  status: 'pending',
  createdAt: '2026-09-03T12:00:00Z',
  expiresAt: '2026-09-04T12:00:00Z',
};
const bad = [
  { ...good, status: 'unknown' },
  { ...good, responseStatus: 2 ** 31 },
  { ...good, createdAt: 'not-a-date' },
  { ...good, unexpected: true },
  { ...good, id: undefined },
];
for (const authority of authorities) {
  const runtimePath = path.join(
    output,
    authority,
    'typescript',
    'idempotency_record.mjs',
  );
  const runtime = await import(`${pathToFileURL(runtimePath).href}?lane=${authority}`);
  assert.equal(runtime.isIdempotencyRecord(good), true);
  for (const fixture of bad) {
    assert.equal(runtime.isIdempotencyRecord(fixture), false);
  }
}

const grpc = JSON.parse(await read('typespec', 'grpc/projection.json'));
assert.equal(grpc.authority, 'typespec');
assert.equal(grpc.messagesOnly, true);
assert.deepEqual(grpc.operations, []);
const openapi = JSON.parse(
  await read('json-schema-openapi', 'openapi/idempotency_record.openapi.json'),
);
assert.equal(openapi['x-ores-authority'], 'json-schema-openapi');
assert.equal(openapi['x-ores-no-invented-operations'], true);
assert.deepEqual(openapi.paths, {});

console.log(
  'generated polyglot validation passed: independent SQL, types, runtime code, ORM witnesses, and transport projections',
);
