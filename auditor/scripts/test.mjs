import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { auditRoot, root, artifactPath } from "../runner/evidence.mjs";
if (!process.argv[2]?.startsWith(".runs/"))
  throw Error("Test artifacts must stay under auditor/.runs");
const output = artifactPath(process.argv[2]);
const tests = fs
  .readdirSync(path.join(auditRoot, "tests"))
  .filter((f) => f.endsWith(".test.mjs"))
  .sort()
  .map((f) => path.join(auditRoot, "tests", f));
const result = spawnSync(process.execPath, ["--test", ...tests], {
  cwd: root,
  encoding: "utf8",
  timeout: 120000,
  maxBuffer: 4 * 1024 * 1024,
});
fs.writeFileSync(
  output,
  (result.stdout ?? "") +
    (result.stderr ?? "") +
    (result.error ? result.error.message : ""),
  { mode: 0o600 },
);
console.log(
  JSON.stringify({
    exit: result.status,
    artifact: path.relative(auditRoot, output),
  }),
);
process.exitCode = result.status === 0 ? 0 : 1;
