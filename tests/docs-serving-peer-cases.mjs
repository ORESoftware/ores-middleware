// Independent semantic witness. Do not derive expected verdicts from a schema.
export const declarations = [
  'DocsAction', 'DocsDecision', 'DocsHeaders', 'DocsRepresentation', 'DocsRequest',
];
export function docsPeerCases() {
  const cases = [];
  const add = (declaration, id, valid, value) => cases.push({ declaration, id, valid, value });
  const request = { method: 'GET', path: '/api-docs' };
  const decision = { action: 'serve', headOnly: false, headers: {} };
  const actions = ['pass', 'serve', 'method-not-allowed', 'not-acceptable', 'stopped-for-evaluation'];
  const representations = ['html', 'catalog', 'openapi', 'openrpc', 'connect', 'hyper-schema'];
  for (const value of actions) add('DocsAction', value, true, value);
  for (const value of representations) add('DocsRepresentation', value, true, value);
  for (const decl of ['DocsAction', 'DocsRepresentation']) {
    for (const [i, value] of [null, 0, false, {}, [], '', 'UNKNOWN'].entries()) add(decl, `invalid-${i}`, false, value);
  }
  add('DocsRequest', 'minimal', true, request);
  add('DocsRequest', 'empty-strings-are-permitted', true, { method: '', path: '' });
  add('DocsRequest', 'all-optional-fields', true, { ...request, accept: '*/*', format: '', runtimeContractDigest: 'a'.repeat(64), docsContractDigest: '0'.repeat(64) });
  for (const field of ['method', 'path']) {
    const value = { ...request }; delete value[field];
    add('DocsRequest', `missing-${field}`, false, value);
  }
  for (const field of ['method', 'path', 'accept', 'format', 'runtimeContractDigest', 'docsContractDigest']) {
    for (const [i, bad] of [null, 1, false, [], {}].entries()) add('DocsRequest', `${field}-wrong-type-${i}`, false, { ...request, [field]: bad });
  }
  for (const field of ['runtimeContractDigest', 'docsContractDigest']) {
    for (const [i, bad] of ['', 'a'.repeat(63), 'a'.repeat(65), 'A'.repeat(64), 'g'.repeat(64)].entries()) add('DocsRequest', `${field}-bad-pattern-${i}`, false, { ...request, [field]: bad });
  }
  add('DocsRequest', 'unknown-property', false, { ...request, extra: true });
  add('DocsDecision', 'minimal', true, decision);
  for (const status of [0, 200, 65535]) add('DocsDecision', `status-${status}`, true, { ...decision, status });
  for (const [i, status] of [-1, 65536, 1.5, '200', null].entries()) add('DocsDecision', `bad-status-${i}`, false, { ...decision, status });
  for (const field of ['action', 'headOnly', 'headers']) {
    const value = { ...decision }; delete value[field];
    add('DocsDecision', `missing-${field}`, false, value);
  }
  add('DocsDecision', 'true-head', true, { ...decision, headOnly: true });
  for (const [i, headOnly] of [null, 0, 'false', {}, []].entries()) add('DocsDecision', `bad-head-${i}`, false, { ...decision, headOnly });
  add('DocsDecision', 'unknown-action', false, { ...decision, action: 'allow' });
  add('DocsDecision', 'unknown-representation', false, { ...decision, representation: 'yaml' });
  add('DocsDecision', 'known-representation', true, { ...decision, representation: 'openapi' });
  add('DocsDecision', 'unknown-property', false, { ...decision, extra: true });
  const goodHeaders = [{}, { 'content-type': 'application/json' }, { '': '', 'x-unicode': 'é😀' }, JSON.parse('{"__proto__":"ok","constructor":"ok"}')];
  for (const [i, headers] of goodHeaders.entries()) {
    add('DocsHeaders', `valid-${i}`, true, headers);
    add('DocsDecision', `headers-valid-${i}`, true, { ...decision, headers });
  }
  const badHeaders = [null, [], 'x: y', 5, true, { x: 1 }, { x: null }, { x: false }, { x: [] }, { x: {} }, JSON.parse('{"__proto__":1}')];
  for (const [i, headers] of badHeaders.entries()) {
    add('DocsHeaders', `invalid-${i}`, false, headers);
    add('DocsDecision', `headers-invalid-${i}`, false, { ...decision, headers });
  }
  for (const declaration of ['DocsRequest', 'DocsDecision']) {
    for (const [i, value] of [null, [], '', 0, true].entries()) add(declaration, `not-object-${i}`, false, value);
  }
  return cases;
}
