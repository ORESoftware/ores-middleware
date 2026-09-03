#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import path from 'node:path';

const [fixtureArgument, generatedArgument, authority] = process.argv.slice(2);
if (!fixtureArgument || !generatedArgument || !authority) {
  throw new Error(
    'usage: typescript-witness.mjs <fixture.json> <generated.mjs> <authority>',
  );
}

const fixturePath = path.resolve(fixtureArgument);
const generatedPath = path.resolve(generatedArgument);
const fixture = JSON.parse(await readFile(fixturePath, 'utf8'));
const generated = await import(
  `${pathToFileURL(generatedPath).href}?authority=${encodeURIComponent(authority)}`
);

if (typeof generated.isIdempotencyRecord !== 'function') {
  throw new Error('generated TypeScript runtime must export isIdempotencyRecord');
}

const normalize = (value) => {
  const normalized = {};
  for (const field of fixture.wireFields) {
    if (Object.hasOwn(value, field)) {
      normalized[field] = value[field];
    }
  }
  return normalized;
};

const cases = fixture.cases.map((testCase) => {
  const accepted = generated.isIdempotencyRecord(testCase.value);
  return {
    id: testCase.id,
    accepted,
    normalized: accepted ? normalize(testCase.value) : null,
  };
});

const statusAcceptance = Object.fromEntries(
  [...fixture.statuses, '__unknown__'].map((status) => {
    const value = {
      ...fixture.cases.find((entry) => entry.id === 'valid-minimal').value,
      status,
    };
    return [status, generated.isIdempotencyRecord(value)];
  }),
);

process.stdout.write(
  `${JSON.stringify({
    schema: 'ores.generated-runtime-witness/v1',
    authority,
    language: 'typescript',
    model: fixture.model,
    wireFields: fixture.wireFields,
    requiredFields: fixture.requiredFields,
    optionalFields: fixture.optionalFields,
    statuses: fixture.statuses,
    statusAcceptance,
    cases,
  })}\n`,
);
