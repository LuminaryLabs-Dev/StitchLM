import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { pathToFileURL } from "node:url";
import Ajv from "ajv";
import { action } from "./actions.mjs";
import { stop, request, metrics, descendants } from "./lifecycle.mjs";
import {
  auditRoot,
  artifactPath,
  root,
  inside,
  safeId,
  Blocked,
  save,
  sha,
} from "./evidence.mjs";
const schema = JSON.parse(
  fs.readFileSync(path.join(auditRoot, "schema/scenario.schema.json")),
);
const validate = new Ajv({ allErrors: true, strict: false }).compile(schema);
export function load(file) {
  const rows = fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter((x) => x.trim())
    .map((line, i) => {
      try {
        return JSON.parse(line);
      } catch {
        throw Error(`Invalid JSON at line ${i + 1}`);
      }
    });
  if (!validate(rows))
    throw Error("Scenario schema: " + JSON.stringify(validate.errors));
  const header = rows[0];
  safeId(header.id);
  const ids = new Set();
  const walk = (steps) => {
    for (const s of steps) {
      if (s.do === "generate") {
        if (s.request?.local_only && s.request.connector_count !== 0)
          throw Error(
            "Host-only reference must explicitly request zero connectors",
          );
        if (ids.has(s.id)) throw Error("Duplicate generation ID");
        ids.add(s.id);
      }
      if (s.steps) walk(s.steps);
    }
  };
  walk(rows.slice(1));
  return { header, steps: rows.slice(1) };
}
export async function run(
  file,
  { model = "lfm", output, execute = action } = {},
) {
  const { header, steps } = load(file);
  if (!header.models.includes(model))
    throw Error("Scenario does not support selected model");
  const controller = new AbortController();
  const start = performance.now();
  const ctx = {
    model,
    outputDir: output ? path.dirname(output) : null,
    signal: controller.signal,
    origins: new Set(),
    actors: new Map(),
    children: [],
    pending: new Map(),
    declaredFaults: [],
    values: {},
    events: [],
    secrets: new Set(),
    measurements: {},
  };
  ctx.event = (type, data = {}) => {
    if (ctx.events.length >= 50000) throw Error("Event budget exceeded");
    ctx.events.push({
      at_ms: Math.round(performance.now() - start),
      type,
      ...data,
    });
  };
  ctx.signal.addEventListener(
    "abort",
    () => {
      ctx.browserClosing ??= ctx.browser?.close();
      ctx.browserClosing?.catch(() => {});
    },
    { once: true },
  );
  ctx.api = (p, data, auth = "admin") =>
    request(ctx.base + p, {
      origins: ctx.origins,
      signal: ctx.signal,
      method: data === undefined ? "GET" : "POST",
      data,
      token:
        auth === "none"
          ? ""
          : auth === "wrong"
            ? "invalid-pairing-code"
            : auth === "pair"
              ? ctx.pair
              : ctx.admin,
    });
  let status = "PASS",
    error = null;
  const timeout = setTimeout(
    () => controller.abort(Error("Scenario deadline exceeded")),
    header.timeout_ms,
  );
  const sample = setInterval(() => {
    for (const p of [
      ...ctx.children,
      ...descendants(process.pid)
        .filter((p) => /chrome|chromium/.test(p.name))
        .map((p) => ({ ...p, browser: true })),
    ]) {
      const m = metrics(p);
      if (m) {
        const old = ctx.measurements[p.pid];
        ctx.measurements[p.pid] = {
          role: p.browser
            ? "browser-process"
            : p === ctx.host
              ? "host"
              : "native-connector",
          ...m,
          first_sample_ms: old?.first_sample_ms ?? m.sampled_at_ms,
          first_cpu_ticks: old?.first_cpu_ticks ?? m.cpu_ticks,
          max_rss_bytes: Math.max(m.rss_bytes, old?.max_rss_bytes ?? 0),
          max_virtual_bytes: Math.max(
            m.virtual_bytes,
            old?.max_virtual_bytes ?? 0,
          ),
        };
      }
    }
  }, 100);
  const signals = () => controller.abort(Error("Audit interrupted"));
  process.once("SIGTERM", signals);
  process.once("SIGINT", signals);
  try {
    for (const s of steps) await execute(ctx, s);
    if (ctx.pending.size) throw Error("Unawaited generation at scenario end");
    for (const r of ctx.declaredFaults)
      if (!r.used || !r.applied)
        throw Error("Declared fault never occurred or completed");
  } catch (e) {
    status = e instanceof Blocked ? "BLOCKED" : "FAIL";
    error = e.message;
  } finally {
    clearTimeout(timeout);
    clearInterval(sample);
    controller.abort(Error("Scenario finished"));
    let cleanup = [];
    for (const a of ctx.actors.values()) {
      try {
        await a.close();
      } catch (e) {
        cleanup.push(e.message);
      }
    }
    try {
      await (ctx.browserClosing ?? ctx.browser?.close());
    } catch (e) {
      cleanup.push(e.message);
    }
    for (const p of [...ctx.children].reverse())
      try {
        await stop(p);
      } catch (e) {
        cleanup.push(e.message);
      }
    await Promise.allSettled([...ctx.pending.values()]);
    process.removeListener("SIGTERM", signals);
    process.removeListener("SIGINT", signals);
    if (cleanup.length) {
      status = "FAIL";
      error =
        (error ? error + "; " : "") + "Cleanup failed: " + cleanup.join("; ");
    }
    ctx.event("cleanup", { passed: cleanup.length === 0 });
  }
  const report = {
    schema: "stitchlm-audit-result/1",
    id: header.id,
    model,
    status,
    error,
    scenario_sha256: sha(file),
    source_commit: execFileSync("git", ["rev-parse", "HEAD"], {
      cwd: root,
      encoding: "utf8",
    }).trim(),
    source_dirty: !!execFileSync("git", ["status", "--porcelain"], {
      cwd: root,
      encoding: "utf8",
    }).trim(),
    node: process.version,
    host_sha256: fs.existsSync(path.join(root, "target/release/stitchlm-host"))
      ? sha(path.join(root, "target/release/stitchlm-host"))
      : null,
    connector_sha256: fs.existsSync(
      path.join(root, "target/release/stitchlm-connector"),
    )
      ? sha(path.join(root, "target/release/stitchlm-connector"))
      : null,
    wasm_sha256: fs.existsSync(
      path.join(root, "dist/connector-web/pkg/stitchlm_connector_web_bg.wasm"),
    )
      ? sha(
          path.join(
            root,
            "dist/connector-web/pkg/stitchlm_connector_web_bg.wasm",
          ),
        )
      : null,
    duration_ms: Math.round(performance.now() - start),
    simulation: "application-level loopback timing/faults",
    native_cap_bytes: 1000000000,
    browser_cap_enforced: false,
    events: ctx.events,
    values: ctx.values,
    clock_ticks_per_second:
      process.platform === "linux"
        ? Number(
            execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim(),
          )
        : null,
    cpu_duty_target_percent: 10,
    cpu_hard_quota_enforced: false,
    measurements: ctx.measurements,
  };
  if (output) save(output, report, ctx.secrets);
  return report;
}
if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const [file, model, out] = process.argv.slice(2);
  try {
    const f = inside(auditRoot, file);
    if (!out?.startsWith(".runs/"))
      throw Error("Scenario artifacts must stay under auditor/.runs");
    const dest = artifactPath(out);
    const result = await run(f, { model, output: dest });
    console.log(
      JSON.stringify({
        id: result.id,
        status: result.status,
        artifact: path.relative(auditRoot, dest),
      }),
    );
    process.exitCode =
      result.status === "PASS" ? 0 : result.status === "BLOCKED" ? 2 : 1;
  } catch (e) {
    console.error(JSON.stringify({ status: "FAIL", error: e.message }));
    process.exitCode = 1;
  }
}
