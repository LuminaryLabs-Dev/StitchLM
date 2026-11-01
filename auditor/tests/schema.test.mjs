import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { load } from "../runner/run-scenario.mjs";
import { auditRoot } from "../runner/evidence.mjs";
for (const file of fs.readdirSync(path.join(auditRoot, "scenarios")))
  test("valid " + file, () =>
    assert.ok(load(path.join(auditRoot, "scenarios", file)).steps.length),
  );
function invalid(rows) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "audit-schema-"));
  const f = path.join(dir, "case.jsonl");
  fs.writeFileSync(f, rows.map((x) => JSON.stringify(x)).join("\n"));
  try {
    assert.throws(() => load(f));
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
}
const h = { id: "test", name: "test", models: ["fixture"], timeout_ms: 1000 };
test("reject unknown operation", () =>
  invalid([h, { do: "shell", command: "echo hi" }]));
test("reject unknown field", () =>
  invalid([h, { do: "delay", ms: 1, typo: 1 }]));
test("reject traversal ID", () =>
  invalid([
    { ...h, id: "../../bad" },
    { do: "delay", ms: 1 },
  ]));
test("reject negative delay", () => invalid([h, { do: "delay", ms: -1 }]));
test("reject duplicate request IDs", () =>
  invalid([h, { do: "generate", id: "same" }, { do: "generate", id: "same" }]));
test("reject empty scenario", () => invalid([h]));

test("reject host-only reference with remote connector count", () =>
  invalid([
    h,
    {
      do: "generate",
      id: "reference",
      request: { local_only: true, connector_count: 4 },
    },
  ]));
