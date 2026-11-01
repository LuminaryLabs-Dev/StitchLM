import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import { setTimeout as sleep } from "node:timers/promises";
export { sleep };
export async function until(fn, timeout = 30000, signal) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    signal?.throwIfAborted();
    const v = await fn();
    if (v) return v;
    await sleep(20, undefined, { signal });
  }
  throw Error(`Condition timed out after ${timeout} ms`);
}
export function owned(command, args, options = {}) {
  const p = spawn(command, args, {
    ...options,
    detached: process.platform !== "win32",
    stdio: ["ignore", "pipe", "pipe"],
  });
  p.output = "";
  p.on("error", (e) => {
    p.spawnError = e;
  });
  for (const stream of [p.stdout, p.stderr])
    stream.on("data", (b) => {
      p.output = (p.output + b.toString()).slice(-65536);
    });
  return p;
}
export async function stop(p) {
  if (p) p.auditStopped = true;
  if (!p || p.exitCode !== null || p.signalCode) return;
  const done = new Promise((r) => p.once("close", r));
  const kill = (s) => {
    try {
      if (process.platform !== "win32") process.kill(-p.pid, s);
      else p.kill(s);
    } catch (e) {
      if (e.code !== "ESRCH") throw e;
    }
  };
  kill("SIGTERM");
  await Promise.race([done, sleep(2000)]);
  if (p.exitCode === null && !p.signalCode) {
    kill("SIGKILL");
    await Promise.race([done, sleep(2000)]);
  }
  if (p.exitCode === null && !p.signalCode)
    throw Error("Owned process failed to stop");
}
export async function listen(server) {
  await new Promise((r, j) => {
    server.once("error", j);
    server.listen(0, "127.0.0.1", r);
  });
  return `http://127.0.0.1:${server.address().port}`;
}
export async function freePort() {
  const s = http.createServer();
  const u = await listen(s);
  await closeServer(s);
  return new URL(u).port;
}
export async function closeServer(s) {
  s.closeAllConnections();
  await new Promise((r) => s.close(r));
}
export function metrics(p) {
  try {
    const s = fs.readFileSync(`/proc/${p.pid}/status`, "utf8");
    const stat = fs.readFileSync(`/proc/${p.pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    return {
      cpu_ticks: Number(fields[11]) + Number(fields[12]),
      sampled_at_ms: performance.now(),
      rss_bytes: Number(s.match(/VmRSS:\s+(\d+)/)?.[1] ?? 0) * 1024,
      virtual_bytes: Number(s.match(/VmSize:\s+(\d+)/)?.[1] ?? 0) * 1024,
    };
  } catch {
    return null;
  }
}
export function localURL(url, origins) {
  const u = new URL(url);
  if (
    u.protocol !== "http:" ||
    u.hostname !== "127.0.0.1" ||
    u.username ||
    u.password ||
    !origins.has(u.origin)
  )
    throw Error("Destination is not an owned loopback origin");
  return u;
}
export async function request(
  url,
  { origins, signal, method = "GET", token = "", data } = {},
) {
  localURL(url, origins);
  const response = await fetch(url, {
    method,
    signal,
    redirect: "manual",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: data === undefined ? undefined : JSON.stringify(data),
  });
  if (response.status >= 300 && response.status < 400)
    throw Error("Redirect refused");
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  return { status: response.status, body };
}

export function descendants(pid) {
  const all = [];
  try {
    for (const name of fs.readdirSync("/proc")) {
      if (!/^\d+$/.test(name)) continue;
      try {
        const line = fs.readFileSync(`/proc/${name}/stat`, "utf8");
        const fields = line.slice(line.lastIndexOf(")") + 2).split(" ");
        all.push({
          pid: Number(name),
          ppid: Number(fields[1]),
          name: line.slice(line.indexOf("(") + 1, line.lastIndexOf(")")),
        });
      } catch {}
    }
  } catch {}
  const found = new Set([pid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const p of all)
      if (found.has(p.ppid) && !found.has(p.pid)) {
        found.add(p.pid);
        changed = true;
      }
  }
  return all.filter((p) => p.pid !== pid && found.has(p.pid));
}

// Slow inference can legitimately be silent for longer than fetch/Undici's
// implicit body timeout. The scenario AbortSignal owns the explicit deadline.
export async function streamedRequest(
  url,
  { origins, signal, token, data, maxBytes = 4194304 },
) {
  const target = localURL(url, origins);
  return new Promise((resolve, reject) => {
    const req = http.request(
      target,
      {
        method: "POST",
        signal,
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
        },
      },
      (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400) {
          res.destroy();
          reject(Error("Redirect refused"));
          return;
        }
        const chunks = [];
        let bytes = 0;
        res.on("data", (chunk) => {
          bytes += chunk.length;
          if (bytes > maxBytes) {
            res.destroy(Error("Response exceeds evidence budget"));
            return;
          }
          chunks.push(chunk);
        });
        res.on("error", reject);
        res.on("aborted", () =>
          reject(Error("Inference response interrupted")),
        );
        res.on("end", () =>
          resolve({
            status: res.statusCode,
            text: Buffer.concat(chunks).toString("utf8"),
          }),
        );
      },
    );
    req.on("error", reject);
    req.end(JSON.stringify(data));
  });
}
