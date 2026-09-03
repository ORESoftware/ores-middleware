import fs from "node:fs";
import Ajv2020 from "ajv/dist/2020.js";

const schema = JSON.parse(
  fs.readFileSync(new URL("../contracts/docs-serving.schema.json", import.meta.url), "utf8"),
);
const ajv = new Ajv2020({ strict: true, allErrors: true });
ajv.compile(schema);
console.log("docs-serving Draft 2020-12 schema is valid");
