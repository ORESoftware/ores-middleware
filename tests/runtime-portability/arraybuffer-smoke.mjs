function fail(message) {
  throw new Error(message);
}

const buffer = new ArrayBuffer(8);
const view = new DataView(buffer);
view.setUint32(0, 0x12345678, false);
view.setUint32(4, 0x90abcdef, true);
if (view.getUint32(0, false) !== 0x12345678) fail("big-endian DataView semantics drifted");
if (view.getUint32(4, true) !== 0x90abcdef) fail("little-endian DataView semantics drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "arraybuffer",
  status: "passed",
}));
