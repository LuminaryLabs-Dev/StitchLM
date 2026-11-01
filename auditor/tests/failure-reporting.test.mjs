import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { run } from "../runner/run-scenario.mjs";
import { Blocked } from "../runner/evidence.mjs";
async function trial(step, execute, timeout = 1000) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "audit-failure-"));
  const f = path.join(dir, "case.jsonl");
  fs.writeFileSync(
    f,
    JSON.stringify({
      id: "failure",
      name: "fixture only",
      models: ["fixture"],
      timeout_ms: timeout,
    }) +
      "\n" +
      JSON.stringify(step) +
      "\n",
  );
  try {
    return await run(f, {
      model: "fixture",
      output: path.join(dir, "result.json"),
      ...(execute ? { execute } : {}),
    });
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
}
test("wrong assertion fails", async () =>
  assert.equal(
    (await trial({ do: "assert", from: "missing", expect: [{ equals: 3 }] }))
      .status,
    "FAIL",
  ));
test("deadline fails and cleans up", async () => {
  const r = await trial({ do: "delay", ms: 1000 }, null, 100);
  assert.equal(r.status, "FAIL");
  assert.equal(r.events.at(-1).type, "cleanup");
});
test("missing prerequisite is blocked never pass", async () =>
  assert.equal(
    (
      await trial({ do: "delay", ms: 1 }, async () => {
        throw new Blocked("model missing");
      })
    ).status,
    "BLOCKED",
  ));
test("fixture success can pass", async () =>
  assert.equal((await trial({ do: "delay", ms: 1 })).status, "PASS"));

test("NexusAuditor refuses a failing command result", async () => {
  const { spawnSync } = await import("node:child_process");
  const { auditRoot } = await import("../runner/evidence.mjs");
  const python = path.join(auditRoot, ".venv/bin/python");
  assert.ok(
    fs.existsSync(python),
    "Install pinned NexusAuditor before integration tests",
  );
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "audit-nexus-failure-"));
  try {
    fs.writeFileSync(
      path.join(dir, "cases.jsonl"),
      JSON.stringify({
        test: "KNOWN-FAILURE",
        name: "intentional failing fixture",
        suite: "fixture",
        services: [],
      }) +
        "\n" +
        JSON.stringify({
          do: "command",
          command: [process.execPath, "-e", "process.exit(7)"],
          cwd: ".",
        }) +
        "\n",
    );
    fs.writeFileSync(
      path.join(dir, "profile.json"),
      JSON.stringify({
        allowed_origins: ["http://127.0.0.1:8787"],
        scenario_files: ["cases.jsonl"],
        runs_dir: "runs",
        services: {},
      }),
    );
    const result = spawnSync(
      python,
      [
        "-m",
        "nexus_auditor",
        "validate-local",
        "fixture",
        "--target",
        dir,
        "--profile",
        path.join(dir, "profile.json"),
      ],
      { encoding: "utf8" },
    );
    assert.notEqual(result.status, 0);
    const parsed = JSON.parse(result.stdout);
    assert.equal(parsed.status, "FAIL");
    assert.equal(parsed.fail, 1);
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
});

test("assertion failure still terminates its owned child", async () => {
  const { owned } = await import("../runner/lifecycle.mjs");
  let child;
  const r = await trial({ do: "delay", ms: 1 }, async (ctx) => {
    child = owned(process.execPath, ["-e", "setInterval(()=>{},1000)"]);
    ctx.children.push(child);
    throw Error("injected assertion failure");
  });
  assert.equal(r.status, "FAIL");
  assert.ok(child.exitCode !== null || child.signalCode);
  assert.equal(r.events.at(-1).passed, true);
});
test("a fault that never triggers fails acceptance", async () => {
  const r = await trial({ do: "delay", ms: 1 }, async (ctx) => {
    ctx.declaredFaults.push({ used: false, applied: false });
    ctx.actors.set("fixture", {
      rules: [{ used: false }],
      close: async () => {},
    });
  });
  assert.equal(r.status, "FAIL");
  assert.match(r.error, /never occurred/);
});
