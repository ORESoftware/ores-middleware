function fail(message) {
  throw new Error(message);
}

if (typeof FormData === "undefined") fail("FormData is unavailable");
const form = new FormData();
form.append("a", "1");
form.append("a", "2");
form.set("b", "3");
if (form.getAll("a").join(",") !== "1,2") fail("FormData duplicate values drifted");
if (form.get("b") !== "3") fail("FormData set/get drifted");

console.log(JSON.stringify({
  schema: "ores.middleware.js-runtime-portability-smoke/v1",
  suite: "formdata",
  status: "passed",
}));
