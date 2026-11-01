import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";
import { root, auditRoot } from "../runner/evidence.mjs";
export function doctor(suite = "full") {
  const missing = [];
  const python = path.join(auditRoot, ".venv/bin/python");
  if (!fs.existsSync(python))
    missing.push("Run npm run setup to install pinned NexusAuditor");
  else if (
    spawnSync(python, ["-c", "import nexus_auditor"], { encoding: "utf8" })
      .status !== 0
  )
    missing.push("NexusAuditor Python import");
  if (suite !== "fast") {
    for (const f of [
      "target/release/stitchlm-host",
      "target/release/stitchlm-connector",
      "dist/connector-web/pkg/stitchlm_connector_web_bg.wasm",
    ])
      if (!fs.existsSync(path.join(root, f))) missing.push(f);
    if (
      !fs.existsSync(
        process.env.STITCH_AUDIT_BROWSER || chromium.executablePath(),
      )
    )
      missing.push("Playwright browser or STITCH_AUDIT_BROWSER");
    if (
      process.platform !== "linux" ||
      spawnSync("prlimit", ["--version"]).status !== 0
    )
      missing.push("Linux prlimit for native 1 GB cap");
    for (const model of suite === "full"
      ? ["LFM", "BONSAI"]
      : [suite.toUpperCase()]) {
      const directory = process.env["STITCH_AUDIT_" + model];
      if (!directory || !fs.existsSync(directory)) {
        missing.push("STITCH_AUDIT_" + model);
        continue;
      }
      const files =
        model === "LFM"
          ? ["config.json", "tokenizer.json", "model.safetensors"]
          : [
              "Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf",
              "tokenizer.json",
            ];
      for (const file of files)
        if (!fs.existsSync(path.join(directory, file)))
          missing.push(model + ": " + file);
    }
  }
  return { status: missing.length ? "BLOCKED" : "PASS", missing };
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const r = doctor(process.argv[2]);
  console.log(JSON.stringify(r, null, 2));
  process.exitCode = r.status === "PASS" ? 0 : 2;
}
