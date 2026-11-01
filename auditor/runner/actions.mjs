import fs from "node:fs";
import path from "node:path";
import assert from "node:assert/strict";
import {
  owned,
  stop,
  freePort,
  until,
  sleep,
  request,
  localURL,
  streamedRequest,
} from "./lifecycle.mjs";
import { root, Blocked } from "./evidence.mjs";
import { join, leave } from "./actors.mjs";
import { generation, check } from "./assertions.mjs";
export async function hostStart(ctx) {
  if (ctx.host) throw Error("Host already started");
  const model = ctx.model;
  const dir =
    model === "lfm"
      ? process.env.STITCH_AUDIT_LFM
      : process.env.STITCH_AUDIT_BONSAI;
  const bin = path.join(root, "target/release/stitchlm-host");
  if (!fs.existsSync(bin)) throw new Blocked("Build the native host first");
  if (!dir || !fs.existsSync(dir))
    throw new Blocked(`Missing ${model} model directory`);
  const required =
    model === "lfm"
      ? ["config.json", "tokenizer.json", "model.safetensors"]
      : ["Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf", "tokenizer.json"];
  for (const file of required)
    if (!fs.existsSync(path.join(dir, file)))
      throw new Blocked("Missing " + model + " input: " + file);
  const port = await freePort();
  ctx.base = `http://127.0.0.1:${port}`;
  ctx.origins.add(ctx.base);
  const args =
    model === "lfm"
      ? ["--model-dir", dir]
      : [
          "--model-file",
          path.join(dir, "Ternary-Bonsai-27B-Abliterated-LowDeg-Q2_0.gguf"),
          "--tokenizer",
          path.join(dir, "tokenizer.json"),
        ];
  ctx.host = owned(
    bin,
    [
      ...args,
      "--listen",
      `127.0.0.1:${port}`,
      "--context",
      model === "lfm" ? "256" : "64",
      "--web-dir",
      path.join(root, "dist"),
    ],
    {
      cwd: root,
      env: {
        ...process.env,
        RAYON_NUM_THREADS: "2",
        TOKIO_WORKER_THREADS: "2",
      },
    },
  );
  ctx.children.push(ctx.host);
  await until(
    () => {
      if (ctx.host.spawnError) throw ctx.host.spawnError;
      if (ctx.host.exitCode !== null)
        throw Error("Host exited before readiness");
      const m = ctx.host.output.match(
        /Host access code: (\w+)\s+Pairing code: (\w+)/,
      );
      if (!m) return false;
      [ctx.admin, ctx.pair] = m.slice(1);
      ctx.secrets.add(ctx.admin);
      ctx.secrets.add(ctx.pair);
      return true;
    },
    180000,
    ctx.signal,
  );
  await until(
    async () => {
      try {
        return (await ctx.api("/api/status")).status === 200;
      } catch {
        return false;
      }
    },
    10000,
    ctx.signal,
  );
  ctx.event("host-ready", {
    model,
    identity: (await ctx.api("/api/status")).body.model,
  });
}
async function generate(ctx, s) {
  const payload = {
    prompt: ctx.model === "bonsai" ? "Hello" : "Reply with the word hello.",
    max_tokens: ctx.model === "bonsai" ? 1 : 12,
    raw_prompt: ctx.model === "bonsai",
    local_only: false,
    connector_count: Math.min(ctx.actors.size, 4),
    split_axis: "rows",
    working_memory_bytes: 32000000,
    ...s.request,
  };
  const promise = (async () => {
    if (s.via === "console") {
      if (!ctx.browser) throw Error("Console journey requires a browser actor");
      const context = await ctx.browser.newContext({ serviceWorkers: "block" });
      ctx.consoleContexts ??= [];
      ctx.consoleContexts.push(context);
      await context.routeWebSocket("**/*", (socket) => socket.close());
      await context.route("**/*", (route) => {
        try {
          localURL(route.request().url(), ctx.origins);
          return route.continue();
        } catch {
          return route.abort();
        }
      });
      const page = await context.newPage();
      await page.addInitScript(() => {
        const original = window.fetch.bind(window);
        window.fetch = async (...args) => {
          const response = await original(...args);
          if (new URL(response.url).pathname === "/api/generate") {
            window.__auditGeneration = response
              .clone()
              .text()
              .then((text) => ({ status: response.status, text }));
          }
          return response;
        };
      });
      await page.goto(ctx.base + "/host-web/");
      await page.locator("#access").fill(ctx.admin);
      await page.locator("#refresh").click();
      await page.waitForFunction(
        () =>
          document.querySelector("#status").textContent === "Host connected",
      );
      await page.locator("#prompt").fill(payload.prompt);
      await page.locator("#tokens").fill(String(payload.max_tokens));
      await page.getByText("Generation options", { exact: true }).click();
      await page
        .locator("#prompt-mode")
        .selectOption(payload.raw_prompt ? "raw" : "chat");
      await page.locator("#generation-axis").selectOption(payload.split_axis);
      await page.locator("#generate").click();
      await page.waitForFunction(
        () =>
          /^(done|error) \|/.test(
            document.querySelector("#status").textContent,
          ),
        null,
        { timeout: ctx.model === "bonsai" ? 1700000 : 120000 },
      );
      const captured = await page.evaluate(() => window.__auditGeneration);
      if (!captured) throw Error("No observed console response");
      const text = captured.text;
      ctx.event("console-result", {
        text: await page.locator("#output").innerText(),
        status: await page.locator("#status").innerText(),
      });
      await page.locator("#access").fill("");
      if (ctx.outputDir)
        await page.screenshot({
          path: path.join(ctx.outputDir, s.id + "-console.png"),
          fullPage: true,
        });
      await context.close();
      return {
        status: captured.status,
        events: text
          .trim()
          .split("\n")
          .filter(Boolean)
          .map((x) => JSON.parse(x)),
      };
    }
    localURL(ctx.base, ctx.origins);
    const r = await streamedRequest(ctx.base + "/api/generate", {
      origins: ctx.origins,
      signal: ctx.signal,
      token: ctx.admin,
      data: payload,
    });
    const text = r.text;
    if (r.status !== 200) return { status: r.status, events: [], body: text };
    return {
      status: r.status,
      events: text
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((x) => JSON.parse(x)),
    };
  })();
  promise.catch(() => {});
  ctx.pending.set(s.id, promise);
  ctx.event("generation-start", { id: s.id });
  if (!s.background) await finish(ctx, s);
}
async function finish(ctx, s) {
  const p = ctx.pending.get(s.id);
  if (!p) throw Error("Unknown generation");
  const result = await p;
  ctx.values[s.id] = result;
  ctx.pending.delete(s.id);
  if (s.expect_status !== undefined)
    assert.equal(result.status, s.expect_status);
  else assert.equal(result.status, 200);
  if (s.expect) generation(result.events, s.expect);
  ctx.event("generation-end", {
    id: s.id,
    status: result.status,
    last: result.events.at(-1),
  });
}
export async function action(ctx, s) {
  ctx.signal.throwIfAborted();
  ctx.event("step-start", { do: s.do, id: s.id });
  switch (s.do) {
    case "host.start":
      await hostStart(ctx);
      break;
    case "host.restart": {
      const old = ctx.admin;
      const oldPair = ctx.pair;
      for (const name of [...ctx.actors.keys()]) await leave(ctx, name);
      await stop(ctx.host);
      ctx.origins.delete(ctx.base);
      ctx.host = null;
      await hostStart(ctx);
      const stale = await request(ctx.base + "/api/status", {
        origins: ctx.origins,
        signal: ctx.signal,
        token: old,
      });
      assert.equal(stale.status, 401);
      const stalePair = await request(ctx.base + "/api/register", {
        origins: ctx.origins,
        signal: ctx.signal,
        method: "POST",
        token: oldPair,
        data: {
          protocol: 1,
          name: "stale",
          kind: "native",
          memory_budget_mb: 953,
        },
      });
      assert.equal(stalePair.status, 401);
      ctx.event("old-credential-rejected", { status: stale.status });
      break;
    }
    case "join":
      await join(ctx, s);
      break;
    case "leave":
      await leave(ctx, s.actor);
      break;
    case "parallel":
      await Promise.all(s.steps.map((x) => action(ctx, x)));
      break;
    case "delay":
      await sleep(s.ms, undefined, { signal: ctx.signal });
      break;
    case "generate":
      await generate(ctx, s);
      break;
    case "await":
      await finish(ctx, s);
      break;
    case "cancel": {
      const r = await ctx.api("/api/cancel", {});
      assert.equal(r.status, 200);
      break;
    }
    case "wait.active":
      await until(
        async () => !!(await ctx.api("/api/status")).body.active_request,
        s.timeout_ms ?? 30000,
        ctx.signal,
      );
      break;
    case "wait.ready":
      await until(
        async () => {
          const cs = (await ctx.api("/api/status")).body.connectors;
          return cs.length === s.count && cs.every((c) => c.ready);
        },
        s.timeout_ms ?? 120000,
        ctx.signal,
      );
      break;
    case "fault": {
      const a = ctx.actors.get(s.actor);
      if (!a) throw Error("Unknown actor");
      if (a.rules.some((r) => r.id === s.id)) throw Error("Duplicate fault ID");
      const rule = { ...s, used: false, applied: false };
      a.rules.push(rule);
      ctx.declaredFaults.push(rule);
      break;
    }
    case "wait.event":
      await until(
        () =>
          ctx.events.some(
            (e) =>
              e.type === s.type &&
              (!s.actor || e.actor === s.actor) &&
              (!s.id || e.id === s.id),
          ),
        s.timeout_ms ?? 30000,
        ctx.signal,
      );
      break;
    case "release": {
      const a = ctx.actors.get(s.actor);
      const release = a?.held.get(s.id);
      if (!release) throw Error("Fault not held");
      release();
      a.held.delete(s.id);
      break;
    }
    case "api": {
      const r = await ctx.api(s.path, s.data, s.auth);
      if (s.save) ctx.values[s.save] = r;
      for (const rule of s.expect) check(r, rule);
      break;
    }
    case "assert": {
      for (const rule of s.expect) check(ctx.values[s.from], rule);
      break;
    }
    case "assert.tokens": {
      const a = ctx.values[s.left]?.events
        ?.filter((e) => e.kind === "token")
        .map((e) => e.token);
      const b = ctx.values[s.right]?.events
        ?.filter((e) => e.kind === "token")
        .map((e) => e.token);
      assert.ok(a?.length && b?.length, "Missing token evidence");
      assert.deepEqual(a, b);
      break;
    }
    case "assert.participation": {
      const actors = [...ctx.actors.values()].map((a) => ({
        name: a.name,
        kind: a.kind,
        matrix_weight_fetches: a.matrixFetches ?? 0,
      }));
      assert.ok(
        actors.filter((a) => a.matrix_weight_fetches > 0).length >= s.minimum,
        "Insufficient observed matrix participants",
      );
      ctx.event("matrix-participation", { actors });
      break;
    }
    case "assert.alive":
      for (const p of ctx.children)
        if (!p.auditStopped)
          assert.equal(p.exitCode, null, "Owned process exited unexpectedly");
      break;
    case "assert.event": {
      const count = ctx.events.filter(
        (e) =>
          e.type === s.type &&
          (!s.actor || e.actor === s.actor) &&
          (!s.id || e.id === s.id) &&
          (!s.status || e.status === s.status),
      ).length;
      assert.ok(count >= (s.minimum ?? 1), "Expected event was not observed");
      break;
    }
    case "checkpoint.metrics":
      ctx.values[s.save] = structuredClone(ctx.measurements);
      break;
    case "assert.memory": {
      const before = ctx.values[s.from];
      assert.ok(
        before && Object.keys(before).length,
        "No baseline memory samples",
      );
      assert.ok(
        Object.values(before).some((m) => m.role === "host"),
        "Host baseline missing",
      );
      const sum = (values) =>
        Object.values(values).reduce((n, m) => n + m.rss_bytes, 0);
      const growth = sum(ctx.measurements) - sum(before);
      assert.ok(
        growth <= s.max_growth_bytes,
        "Aggregate measured RSS growth exceeds declared budget",
      );
      ctx.event("memory-growth", {
        growth_bytes: growth,
        limit_bytes: s.max_growth_bytes,
      });
      break;
    }
    default:
      throw Error("Unknown action");
  }
  ctx.event("step-end", { do: s.do, id: s.id });
}
