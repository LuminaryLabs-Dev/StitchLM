import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import {
  auditRoot,
  root,
  inside,
  save,
  artifactPath,
} from "../runner/evidence.mjs";
import { load } from "../runner/run-scenario.mjs";
import { doctor } from "./doctor.mjs";
const suite = process.argv[2] ?? "full";
const catalog = JSON.parse(
  fs.readFileSync(path.join(auditRoot, "suites.json")),
);
if (!catalog[suite]) throw Error("Unknown suite");
const preflight = doctor(suite);
if (preflight.status !== "PASS") {
  console.log(JSON.stringify(preflight, null, 2));
  process.exitCode = 2;
} else {
  const run = path.join(
    auditRoot,
    ".runs",
    new Date().toISOString().replace(/[:.]/g, "-") +
      "-" +
      crypto.randomBytes(4).toString("hex"),
  );
  artifactPath(path.relative(auditRoot, run));
  fs.mkdirSync(run, { recursive: true, mode: 0o700 });
  const wrappers = [];
  const selected = [];
  if (suite === "fast") {
    wrappers.push(
      {
        test: "HARNESS",
        name: "Scenario harness unit contracts",
        suite,
        services: [],
      },
      {
        do: "command",
        command: [
          "node",
          "auditor/scripts/test.mjs",
          path.relative(auditRoot, path.join(run, "harness-tests.log")),
        ],
        cwd: ".",
        timeout_seconds: 120,
      },
    );
  } else {
    for (const file of fs
      .readdirSync(path.join(auditRoot, "scenarios"))
      .filter((f) => f.endsWith(".jsonl"))
      .sort()) {
      const { header } = load(inside(auditRoot, "scenarios/" + file));
      for (const model of catalog[suite].models.filter((m) =>
        header.models.includes(m),
      )) {
        const id = header.id + "-" + model;
        if (
          process.env.STITCH_AUDIT_ONLY &&
          header.id !== process.env.STITCH_AUDIT_ONLY
        )
          continue;
        selected.push({ id, file, model });
        wrappers.push(
          { test: id, name: header.name + " " + model, suite, services: [] },
          {
            do: "command",
            command: [
              "node",
              "auditor/runner/run-scenario.mjs",
              "scenarios/" + file,
              model,
              path.relative(auditRoot, path.join(run, id + ".json")),
            ],
            cwd: ".",
            timeout_seconds: Math.ceil(header.timeout_ms / 1000) + 60,
          },
        );
      }
    }
    if (!selected.length) throw Error("No matching scenarios");
  }
  const wrapper = path.join(run, "commands.jsonl");
  fs.writeFileSync(
    wrapper,
    wrappers.map((x) => JSON.stringify(x)).join("\n") + "\n",
  );
  const profile = {
    ...JSON.parse(fs.readFileSync(path.join(auditRoot, "validation.json"))),
    scenario_files: [path.relative(root, wrapper)],
  };
  const profilePath = path.join(run, "validation.json");
  save(profilePath, profile);
  const python = path.join(auditRoot, ".venv/bin/python");
  const r = spawnSync(
    python,
    [
      "-m",
      "nexus_auditor",
      "validate-local",
      suite,
      "--target",
      root,
      "--profile",
      profilePath,
      "--output",
      path.join(run, "nexus"),
    ],
    {
      cwd: root,
      encoding: "utf8",
      timeout: catalog[suite].kind === "harness" ? 180000 : 16 * 1860000,
      maxBuffer: 4 * 1024 * 1024,
    },
  );
  if (r.stdout) process.stdout.write(r.stdout);
  if (r.stderr) process.stderr.write(r.stderr);
  const results = selected.map((s) => {
    const f = path.join(run, s.id + ".json");
    return fs.existsSync(f)
      ? JSON.parse(fs.readFileSync(f))
      : { ...s, status: "FAIL", error: "No scenario result artifact" };
  });
  const status =
    r.status === 0 && results.every((x) => x.status === "PASS")
      ? "PASS"
      : results.some((x) => x.status === "BLOCKED")
        ? "BLOCKED"
        : "FAIL";
  save(path.join(run, "summary.json"), {
    suite,
    status,
    nexus_exit: r.status,
    results: results.map(({ id, status, error, duration_ms }) => ({
      id,
      status,
      error,
      duration_ms,
    })),
  });
  console.log(JSON.stringify({ status, run: path.relative(root, run) }));
  process.exitCode = status === "PASS" ? 0 : status === "BLOCKED" ? 2 : 1;
}
