import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const here = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: here, stdio: "inherit" });
  if (r.error) throw r.error;
  if (r.status !== 0) throw Error(`${cmd} failed with ${r.status}`);
}
try {
  run("python3", ["-m", "venv", path.join(here, ".venv")]);
  const python = path.join(here, ".venv/bin/python");
  const arg = process.argv.indexOf("--auditor-source");
  if (arg !== -1) {
    const source = fs.realpathSync(process.argv[arg + 1]);
    const pin = JSON.parse(
      fs.readFileSync(path.join(here, "nexus-auditor-source-lock.json")),
    );
    for (const f of pin.files) {
      const file = path.join(source, f.path);
      const b = fs.readFileSync(file);
      const actual = crypto
        .createHash("sha1")
        .update(`blob ${b.length}\0`)
        .update(b)
        .digest("hex");
      if (actual !== f.sha)
        throw Error(`NexusAuditor source pin mismatch: ${f.path}`);
    }
    run(python, ["-m", "pip", "install", "--no-deps", source]);
  } else
    run(python, [
      "-m",
      "pip",
      "install",
      "--no-deps",
      "-r",
      "requirements.lock.txt",
    ]);
  run("npm", ["ci", "--ignore-scripts"]);
  if (!process.argv.includes("--skip-browser"))
    run(process.execPath, [
      "node_modules/playwright/cli.js",
      "install",
      "chromium",
    ]);
  console.log(
    "Audit tooling installed; run doctor for model/build prerequisites.",
  );
} catch (e) {
  console.error(e.message);
  process.exitCode = 2;
}
