import {
  CONTRACT_DIGEST_HEADER,
  DOCS_FORMAT_HEADER,
  decideDocs,
} from '../../src/ts/src/docs-serving.js';

const digest = 'a'.repeat(64);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: actual=${JSON.stringify(actual)} expected=${JSON.stringify(expected)}`);
  }
}

const openApi = decideDocs({
  path: '/api/docs',
  method: 'GET',
  accept: 'application/vnd.oai.openapi+json',
  runtimeContractDigest: digest,
  docsContractDigest: digest,
});
assertEqual(openApi.action, 'serve', 'matching contract digests must serve docs');
assertEqual(openApi.status, 200, 'successful docs response status');
assertEqual(openApi.representation, 'openapi', 'accept negotiation must select OpenAPI');
assertEqual(openApi.headers[CONTRACT_DIGEST_HEADER], digest, 'contract digest response header');
assertEqual(openApi.headers['Cache-Control'], 'no-store', 'docs responses must not be cached');

const head = decideDocs({
  path: '/openapi.json',
  method: 'HEAD',
  accept: 'application/openapi+json',
  runtimeContractDigest: digest,
  docsContractDigest: digest,
});
assertEqual(head.action, 'serve', 'HEAD must use the same representation admission');
assertEqual(head.headOnly, true, 'HEAD must suppress the response body');
assertEqual(head.representation, 'openapi', 'fixed OpenAPI route representation');

const mismatchedDigest = decideDocs({
  path: '/api/docs',
  method: 'GET',
  runtimeContractDigest: digest,
  docsContractDigest: 'b'.repeat(64),
});
assertEqual(mismatchedDigest.action, 'stopped-for-evaluation', 'digest drift must fail closed');
assertEqual(mismatchedDigest.status, 503, 'digest drift status');

const invalidDigest = decideDocs({
  path: '/api/docs',
  method: 'GET',
  runtimeContractDigest: 'not-a-sha256',
  docsContractDigest: 'not-a-sha256',
});
assertEqual(invalidDigest.action, 'stopped-for-evaluation', 'invalid digest syntax must fail closed');
assertEqual(invalidDigest.status, 503, 'invalid digest status');

const methodRejected = decideDocs({ path: '/api/docs', method: 'POST' });
assertEqual(methodRejected.action, 'method-not-allowed', 'mutating methods must not serve docs');
assertEqual(methodRejected.status, 405, 'mutating method status');
assertEqual(methodRejected.headers.Allow, 'GET, HEAD', 'allowed methods header');

const formatRejected = decideDocs({
  path: '/api/docs',
  method: 'GET',
  format: 'yaml',
});
assertEqual(formatRejected.action, 'not-acceptable', 'unknown format must fail closed');
assertEqual(formatRejected.status, 406, 'unknown format status');

const fixedRepresentationRejected = decideDocs({
  path: '/openapi.json',
  method: 'GET',
  format: 'openrpc',
});
assertEqual(
  fixedRepresentationRejected.action,
  'not-acceptable',
  'fixed route cannot be overridden to another representation',
);
assertEqual(fixedRepresentationRejected.status, 406, 'fixed representation mismatch status');

const unrelated = decideDocs({ path: '/healthz', method: 'GET' });
assertEqual(unrelated.action, 'pass', 'non-docs routes must pass through');
assertEqual(unrelated.headOnly, false, 'pass-through request body behavior');

const html = decideDocs({
  path: '/api/docs',
  method: 'GET',
  accept: 'text/html',
});
assertEqual(html.action, 'serve', 'HTML docs should serve on the generic route');
assertEqual(html.representation, 'html', 'HTML representation');
assert(
  String(html.headers['Content-Security-Policy']).includes("default-src 'none'"),
  'HTML docs must keep restrictive CSP',
);
assertEqual(html.headers['X-Frame-Options'], 'DENY', 'HTML docs must deny framing');
assert(
  String(html.headers.Vary).includes(DOCS_FORMAT_HEADER),
  'cache variance must include the explicit docs-format header',
);

console.log(JSON.stringify({
  schema: 'ores.middleware.js-runtime-portability-smoke/v1',
  suite: 'docs-serving',
  status: 'passed',
  checks: 9,
}));
