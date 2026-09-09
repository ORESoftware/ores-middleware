import assert from "node:assert/strict";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { validateSourceBindings } from "../scripts/lib/function-body-contracts.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const bindings = JSON.parse(await readFile(path.join(root, "contracts/function-bodies/language-bindings.json"), "utf8"));
const plan = JSON.parse(await readFile(path.join(root, bindings.plan), "utf8"));
const sourcePath = "src/rust/src/operation.rs";
const rust = bindings.bindings.find(binding => binding.language === "rust");
const guard = rust.stepEvidence.find(step => step.stepId === "arm-termination-guard");
const cancellation = "_ = &mut cancellation => BoundaryEvent::Cancelled";
const expiry = "_ = &mut expiry => BoundaryEvent::DeadlineExceeded";
const operation = "outcome = &mut guarded => BoundaryEvent::Outcome(outcome)";

// These are source-binding regressions, not synthetic native runtime verdicts.
// Rust admission and real loopback I/O are executed by the separate Cargo gate.
test("Rust binding requires monotonic deadline and cancellation-first polling", async () => {
  assert.equal(guard.ordered, true);
  assert.deepEqual(guard.requiredFragments, [
    "tokio::time::Instant::now().checked_add(budget)",
    "tokio::time::sleep_until(at).await", "tokio::select!", "biased;",
    cancellation, expiry, operation,
  ]);
  assert.deepEqual((await validateSourceBindings(root, plan, bindings)).findings, []);
});

async function mutatedSources(t, mutate) {
  const directory = await mkdtemp(path.join(tmpdir(), "ores-rust-binding-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  for (const relative of new Set(bindings.bindings.flatMap(binding => binding.sources))) {
    assert.match(relative, /^src\/[A-Za-z0-9_./-]+$/);
    assert.ok(!relative.split("/").includes(".."));
    const target = path.join(directory, relative);
    await mkdir(path.dirname(target), { recursive: true });
    await copyFile(path.join(root, relative), target);
  }
  const target = path.join(directory, sourcePath);
  const original = await readFile(target, "utf8");
  const changed = mutate(original);
  assert.notEqual(changed, original, "negative mutation must actually change the source");
  await writeFile(target, changed);
  const result = await validateSourceBindings(directory, plan, bindings);
  assert.ok(result.findings.some(finding => finding.code === "source-witness-missing" && finding.detail.includes("rust arm-termination-guard")), JSON.stringify(result.findings));
}

function swap(source, left, right) {
  assert.ok(source.includes(left) && source.includes(right));
  const sentinel = "__ORES_UNIT_TEST_SWAP_SENTINEL__";
  assert.ok(!source.includes(sentinel));
  return source.replace(left, sentinel).replace(right, left).replace(sentinel, right);
}

for (const [name, mutate] of [
  ["removing biased polling", source => source.replace("biased;", "")],
  ["polling the handler before cancellation", source => swap(source, cancellation, operation)],
  ["polling the deadline before cancellation", source => swap(source, cancellation, expiry)],
  ["removing deadline observation", source => source.replace("tokio::time::sleep_until(at).await", "std::future::pending::<()>().await")],
  ["removing checked monotonic budget conversion", source => source.replace("tokio::time::Instant::now().checked_add(budget)", "None")],
]) {
  test(`source contract rejects ${name}`, async t => mutatedSources(t, mutate));
}
