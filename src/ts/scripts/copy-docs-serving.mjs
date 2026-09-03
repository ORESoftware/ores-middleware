import { copyFile, mkdir } from "node:fs/promises";

const dist = new URL("../dist/", import.meta.url);
await mkdir(dist, { recursive: true });
await copyFile(new URL("../src/docs-serving.js", import.meta.url), new URL("docs-serving.js", dist));
await copyFile(new URL("../src/docs-serving.d.ts", import.meta.url), new URL("docs-serving.d.ts", dist));
