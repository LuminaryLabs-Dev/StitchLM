import http from "node:http";
import path from "node:path";
import { chromium } from "playwright";
import {
  owned,
  stop,
  listen,
  closeServer,
  until,
  sleep,
  localURL,
} from "./lifecycle.mjs";
import { root, Blocked, safeId } from "./evidence.mjs";
// A loopback forwarding adapter, never a network configuration change.
export async function proxy(ctx, name) {
  const actor = {
    name,
    rules: [],
    held: new Map(),
    records: [],
    sockets: new Set(),
  };
  const server = http.createServer(async (req, res) => {
    try {
      const target = new URL(req.url, ctx.base);
      localURL(target, ctx.origins);
      if (target.origin !== ctx.base) throw Error("Absolute proxy URL refused");
      const parts = [];
      let bytes = 0;
      for await (const p of req) {
        bytes += p.length;
        if (bytes > 16 * 1024 * 1024) throw Error("Request too large");
        parts.push(p);
      }
      const body = Buffer.concat(parts);
      const rule = actor.rules.find(
        (r) =>
          !r.used &&
          target.pathname.includes(r.path) &&
          (!r.method || r.method === req.method),
      );
      if (rule) {
        rule.used = true;
        ctx.event("fault-hit", { actor: name, id: rule.id, mode: rule.mode });
        if (rule.mode === "hold")
          await new Promise((resolve, reject) => {
            actor.held.set(rule.id, resolve);
            ctx.signal.addEventListener(
              "abort",
              () => reject(ctx.signal.reason),
              { once: true },
            );
          });
        if (rule.mode === "delay")
          await sleep(rule.ms, undefined, { signal: ctx.signal });
        if (rule.mode === "abort") {
          req.socket.destroy();
          rule.applied = true;
          ctx.event("fault-applied", { actor: name, id: rule.id });
          return;
        }
      }
      const send = () =>
        new Promise((resolve, reject) => {
          const u = http.request(
            target,
            {
              method: req.method,
              headers: { ...req.headers, host: target.host },
              signal: ctx.signal,
            },
            (up) => {
              if (up.statusCode >= 300 && up.statusCode < 400) {
                up.resume();
                reject(Error("Proxy redirect refused"));
                return;
              }
              resolve(up);
            },
          );
          u.on("error", reject);
          u.end(body);
        });
      const up = await send();
      actor.responseCount = (actor.responseCount ?? 0) + 1;
      if (target.pathname.includes("/shard-weights/") && up.statusCode === 200)
        actor.matrixFetches = (actor.matrixFetches ?? 0) + 1;
      if (rule || up.statusCode >= 400)
        ctx.event("response", {
          actor: name,
          path: target.pathname,
          status: up.statusCode,
        });
      res.writeHead(up.statusCode, up.headers);
      up.pipe(res);
      await new Promise((resolve, reject) => {
        up.on("end", resolve);
        up.on("error", reject);
        res.on("close", resolve);
      });
      if (rule?.mode === "duplicate") {
        const second = await send();
        second.resume();
        ctx.event("duplicate-response", {
          actor: name,
          id: rule.id,
          status: second.statusCode,
        });
        await new Promise((r) => second.on("end", r));
      }
      if (rule) {
        rule.applied = true;
        ctx.event("fault-applied", { actor: name, id: rule.id });
      }
    } catch (e) {
      if (!ctx.signal.aborted)
        ctx.event("proxy-error", { actor: name, message: e.message });
      if (!res.headersSent) res.writeHead(502);
      res.end("audit proxy request failed");
    }
  });
  actor.server = server;
  actor.url = await listen(server);
  ctx.origins.add(actor.url);
  actor.close = async () => {
    for (const release of actor.held.values()) release();
    await closeServer(server);
    ctx.origins.delete(actor.url);
  };
  return actor;
}
export async function join(ctx, s) {
  safeId(s.actor);
  if (ctx.actors.has(s.actor)) throw Error("Actor already exists");
  if (ctx.actors.size >= 4) throw Error("At most four connectors");
  ctx.actors.set(s.actor, { name: s.actor, rules: [], close: async () => {} });
  const a = await proxy(ctx, s.actor);
  ctx.actors.set(s.actor, a);
  a.kind = s.kind;
  const budget = s.memory_mb ?? 953;
  if (s.kind === "native") {
    if (process.platform !== "linux")
      throw new Blocked("Native hard cap requires Linux prlimit");
    a.child = owned(
      "prlimit",
      [
        "--as=1000000000",
        "--",
        path.join(root, "target/release/stitchlm-connector"),
        "--host",
        a.url,
        "--name",
        s.actor,
        "--memory-mb",
        String(budget),
        "--cpu-percent",
        "10",
      ],
      {
        cwd: root,
        env: {
          ...process.env,
          STITCH_PAIRING_CODE: ctx.pair,
          RAYON_NUM_THREADS: "1",
          TOKIO_WORKER_THREADS: "1",
        },
      },
    );
    ctx.children.push(a.child);
  } else {
    if (!ctx.browserPromise) {
      ctx.browserPromise = chromium.launch({
        headless: true,
        ...(process.env.STITCH_AUDIT_BROWSER
          ? { executablePath: process.env.STITCH_AUDIT_BROWSER }
          : {}),
        args: ["--disable-dev-shm-usage"],
      });
    }
    ctx.browser = await ctx.browserPromise;
    a.context = await ctx.browser.newContext({ serviceWorkers: "block" });
    a.context.setDefaultTimeout(30000);
    await a.context.routeWebSocket("**/*", (socket) => socket.close());
    await a.context.route("**/*", (route) => {
      try {
        const u = route.request().url();
        if (u.startsWith("data:") || u.startsWith("blob:"))
          return route.continue();
        localURL(u, ctx.origins);
        return route.continue();
      } catch {
        return route.abort();
      }
    });
    a.page = await a.context.newPage();
    a.page.on("pageerror", (e) =>
      ctx.event("page-error", { actor: s.actor, message: e.message }),
    );
    await a.page.goto(a.url + "/connector-web/");
    await a.page.locator("#pair").fill(ctx.pair);
    await a.page.locator("#name").fill(s.actor);
    await a.page.locator("#memory").fill(String(budget));
    await a.page.locator("#connect").click();
    await a.page.locator("#pair").fill("");
  }
  await until(
    async () => {
      if (a.child?.spawnError) throw a.child.spawnError;
      if (a.child && a.child.exitCode !== null)
        throw Error("Connector exited during join");
      const r = await ctx.api("/api/status");
      const c = r.body.connectors?.find((c) => c.name === s.actor && c.ready);
      if (c) {
        a.id = c.id;
        return true;
      }
      return false;
    },
    120000,
    ctx.signal,
  );
  ctx.event("actor-ready", { actor: s.actor, kind: s.kind });
}
export async function leave(ctx, name) {
  const a = ctx.actors.get(name);
  if (!a) throw Error("Unknown actor");
  if (a.context) await a.context.close();
  await stop(a.child);
  if (a.id) await ctx.api(`/api/connectors/${a.id}/remove`, {});
  await a.close();
  ctx.actors.delete(name);
  ctx.event("actor-left", { actor: name });
}
