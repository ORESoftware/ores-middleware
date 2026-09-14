import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, extname, join, relative, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const roots = ['src', 'docs', 'contracts', 'fixtures'];
const textExtensions = new Set([
  '.d.ts',
  '.erl',
  '.ex',
  '.exs',
  '.gleam',
  '.go',
  '.json',
  '.md',
  '.mjs',
  '.rs',
  '.toml',
  '.ts',
  '.tsp',
  '.txt',
]);

function effectiveExtension(path) {
  return path.endsWith('.d.ts') ? '.d.ts' : extname(path);
}

function walk(path) {
  const stat = statSync(path);
  if (stat.isDirectory()) {
    return readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
      if (entry.name === 'node_modules' || entry.name === 'target' || entry.name === '.git') {
        return [];
      }
      return walk(join(path, entry.name));
    });
  }
  return stat.isFile() && textExtensions.has(effectiveExtension(path)) ? [path] : [];
}

test('authored ORE extension headers use canonical lowercase spelling', () => {
  const violations = roots
    .flatMap((name) => walk(join(root, name)))
    .flatMap((path) => {
      const content = readFileSync(path, 'utf8');
      return /X-Ores-/u.test(content) ? [relative(root, path)] : [];
    });

  assert.deepEqual(
    violations,
    [],
    `mixed-case authored ORE extension headers found in: ${violations.join(', ')}`,
  );
});

test('docs-serving canonical extension names remain lowercase', () => {
  const expected = ['x-ores-docs-format', 'x-ores-contract-sha256'];
  const implementations = [
    'src/rust/src/docs_serving.rs',
    'src/ts/src/docs-serving.js',
    'src/ts/src/docs-serving.d.ts',
    'src/golang/docsserving/docs.go',
    'src/gleam/src/ores_middleware/docs_serving.gleam',
    'docs/architecture.md',
  ];

  for (const path of implementations) {
    const content = readFileSync(join(root, path), 'utf8');
    for (const header of expected) {
      assert.ok(content.includes(header), `${path} must contain ${header}`);
    }
  }
});
