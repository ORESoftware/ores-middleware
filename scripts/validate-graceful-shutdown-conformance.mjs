#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { validateGracefulShutdownConformance } from "./lib/graceful-shutdown-conformance.mjs";

const path = new URL("../contracts/graceful-shutdown/conformance.json", import.meta.url);
const contract = JSON.parse(await readFile(path, "utf8"));
const result = validateGracefulShutdownConformance(contract);

console.log(
  `graceful-shutdown-conformance: ${result.retry} retry, ${result.lifecycle} lifecycle, ${result.admission} admission, and ${result.transport} transport vectors passed`,
);
