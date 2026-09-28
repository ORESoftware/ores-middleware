import { readFileSync } from 'node:fs';

const schema = JSON.parse(readFileSync('contracts/config-fingerprint.schema.json', 'utf8'));
const props = schema.properties;
if (props.schema?.const !== 'ores.middleware.config-fingerprint/v1') throw new Error('schema identity drift');
if (props.fingerprint_sha256?.pattern !== '^[0-9a-f]{64}$') throw new Error('sha256 grammar drift');
for (const key of ['public_field_count', 'secret_binding_count']) {
  if (props[key]?.minimum !== 0 || props[key]?.maximum !== 100000) {
    throw new Error(`${key} bounds drift`);
  }
}
if (props.raw_secret_material_included?.const !== false) throw new Error('secret material invariant drift');
console.log('config fingerprint authored schema invariants passed');
