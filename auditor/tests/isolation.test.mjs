import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import http from "node:http";
import { inside, redactor } from "../runner/evidence.mjs";
import {
  localURL,
  owned,
  stop,
  listen,
  closeServer,
  request,
  until,
} from "../runner/lifecycle.mjs";
import { proxy } from "../runner/actors.mjs";
test("artifact traversal and symlink rejected", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "audit-path-"));
  try {
    fs.symlinkSync("/tmp", path.join(dir, "link"));
    assert.throws(() => inside(dir, "../bad"));
    assert.throws(() => inside(dir, "link/bad"));
  } finally {
    fs.rmSync(dir, { recursive: true });
  }
});
test("only owned loopback origins accepted", () => {
  const allowed = new Set(["http://127.0.0.1:1234"]);
  assert.throws(() => localURL("http://example.com", allowed));
  assert.throws(() => localURL("http://127.0.0.1:99", allowed));
  assert.throws(() => localURL("http://secret@127.0.0.1:1234", allowed));
});
test("redacts credentials", () =>
  assert.equal(
    redactor(new Set(["pair-secret"]))("pair-secret Bearer abc123"),
    "[REDACTED] Bearer [REDACTED]",
  ));
test("redirect refused", async () => {
  const s = http.createServer((q, r) => {
    r.writeHead(302, { location: "http://example.com" });
    r.end();
  });
  const url = await listen(s);
  try {
    await assert.rejects(request(url, { origins: new Set([url]) }), /Redirect/);
  } finally {
    await closeServer(s);
  }
});
test("owned child terminates", async () => {
  const p = owned(process.execPath, ["-e", "setInterval(()=>{},1000)"]);
  await stop(p);
  assert.ok(p.exitCode !== null || p.signalCode);
});
test("hold fault really blocks forwarding until released", async () => {
  let count = 0;
  const s = http.createServer((q, r) => {
    count++;
    r.end("ok");
  });
  const base = await listen(s);
  const controller = new AbortController();
  const ctx = {
    base,
    signal: controller.signal,
    origins: new Set([base]),
    events: [],
    event(type, data) {
      this.events.push({ type, ...data });
    },
  };
  const a = await proxy(ctx, "one");
  a.rules.push({ id: "hold", mode: "hold", path: "/work" });
  try {
    const promise = fetch(a.url + "/work");
    await until(() => a.held.has("hold"));
    assert.equal(count, 0);
    a.held.get("hold")();
    assert.equal(await (await promise).text(), "ok");
    assert.equal(count, 1);
  } finally {
    controller.abort();
    await a.close();
    await closeServer(s);
  }
});

test("streamed inference obeys explicit deadline and size bounds", async () => {
  const { streamedRequest } = await import("../runner/lifecycle.mjs");
  const s = http.createServer((req, res) => {
    req.resume();
    setTimeout(() => res.end("done"), 80);
  });
  const base = await listen(s);
  try {
    const success = await streamedRequest(base, {
      origins: new Set([base]),
      signal: AbortSignal.timeout(1000),
      token: "fixture",
      data: {},
    });
    assert.equal(success.text, "done");
    await assert.rejects(
      streamedRequest(base, {
        origins: new Set([base]),
        signal: AbortSignal.timeout(10),
        token: "fixture",
        data: {},
      }),
    );
    await assert.rejects(
      streamedRequest(base, {
        origins: new Set([base]),
        signal: AbortSignal.timeout(1000),
        token: "fixture",
        data: {},
        maxBytes: 2,
      }),
    );
  } finally {
    await closeServer(s);
  }
});

test("CLI refuses overwriting repository documents with artifacts", async () => {
  const { spawnSync } = await import("node:child_process");
  const { auditRoot, root, sha } = await import("../runner/evidence.mjs");
  const readme = path.join(auditRoot, "README.md");
  const before = sha(readme);
  const result = spawnSync(
    process.execPath,
    [
      "auditor/runner/run-scenario.mjs",
      "scenarios/01-first-generation.jsonl",
      "lfm",
      "README.md",
    ],
    { cwd: root, encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.equal(sha(readme), before);
  assert.match(result.stderr, /artifacts must stay/);
});

test("artifact normalization cannot return to repository files", async () => {
  const { artifactPath, auditRoot } = await import("../runner/evidence.mjs");
  assert.throws(() => artifactPath(".runs/../README.md"));
  const runs = path.join(auditRoot, ".runs");
  fs.mkdirSync(runs, { recursive: true });
  const link = path.join(runs, "unsafe-fixture-link");
  fs.symlinkSync(auditRoot, link);
  try {
    assert.throws(() => artifactPath(".runs/unsafe-fixture-link/README.md"));
  } finally {
    fs.unlinkSync(link);
  }
});
