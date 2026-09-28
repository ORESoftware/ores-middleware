import { readFileSync } from 'node:fs';

const schema = JSON.parse(readFileSync('contracts/operation-boundary.schema.json', 'utf8'));
const rust = readFileSync('src/rust/src/operation.rs', 'utf8');
const ts = readFileSync('src/ts/src/operation.ts', 'utf8');
const go = readFileSync('src/golang/operation.go', 'utf8');

const transports = ['http', 'lambda', 'tcp', 'websocket'];
const scopes = ['request', 'connection', 'message', 'callback'];

const same = (a, b) => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort());
if (!same(schema.$defs.OperationTransport.enum, transports)) throw new Error('JSON Schema transport vocabulary drift');
if (!same(schema.$defs.OperationScope.enum, scopes)) throw new Error('JSON Schema scope vocabulary drift');

for (const value of transports) {
  if (!ts.includes(`"${value}"`)) throw new Error(`TypeScript missing transport ${value}`);
  if (!go.includes(`= "${value}"`)) throw new Error(`Go missing transport ${value}`);
  const rustName = value === 'websocket' ? 'WebSocket' : value[0].toUpperCase() + value.slice(1);
  if (!rust.includes(`Self::${rustName} => "${value}"`)) throw new Error(`Rust missing transport ${value}`);
}
for (const value of scopes) {
  if (!ts.includes(`"${value}"`)) throw new Error(`TypeScript missing scope ${value}`);
  if (!go.includes(`= "${value}"`)) throw new Error(`Go missing scope ${value}`);
  const rustName = value[0].toUpperCase() + value.slice(1);
  if (!rust.includes(`Self::${rustName} => "${value}"`)) throw new Error(`Rust missing scope ${value}`);
}

for (const source of [rust, ts, go]) {
  if (!source.includes('OperationDescriptor')) throw new Error('runtime missing OperationDescriptor');
}
console.log('operation boundary vocabulary matches Rust/TypeScript/Go runtime surfaces');
