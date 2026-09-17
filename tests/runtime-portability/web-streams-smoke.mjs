function fail(message) {
  throw new Error(message);
}

for (const name of ["ReadableStream", "TextEncoder", "TextDecoder"]) {
  if (typeof globalThis[name] === "undefined") fail(`${name} is unavailable`);
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const stream = new ReadableStream({
  start(controller) {
    controller.enqueue(encoder.encode("ores"));
    controller.enqueue(encoder.encode("-middleware"));
    controller.close();
  },
});
const reader = stream.getReader();
let value = "";
for (;;) {
  const item = await reader.read();
  if (item.done) break;
  value += decoder.decode(item.value, { stream: true });
}
value += decoder.decode();
if (value !== "ores-middleware") fail(`stream payload mismatch: ${value}`);

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "web-streams",
  status: "passed",
}));
