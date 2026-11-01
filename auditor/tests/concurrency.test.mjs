import { test } from "node:test";
import assert from "node:assert/strict";
import { action } from "../runner/actions.mjs";
import { sleep } from "../runner/lifecycle.mjs";
test("parallel actions really overlap", async () => {
  const events = [];
  const ctx = {
    signal: new AbortController().signal,
    event: (type, data) => events.push({ type, ...data }),
  };
  await action(ctx, {
    do: "parallel",
    steps: [
      { do: "delay", ms: 80 },
      { do: "delay", ms: 80 },
    ],
  });
  const starts = events
    .map((e, i) => (e.do === "delay" && e.type === "step-start" ? i : -1))
    .filter((i) => i >= 0);
  const ends = events
    .map((e, i) => (e.do === "delay" && e.type === "step-end" ? i : -1))
    .filter((i) => i >= 0);
  assert.equal(starts.length, 2);
  assert.ok(Math.max(...starts) < Math.min(...ends));
});
test("event waits observe event rather than fixed sleep", async () => {
  const c = new AbortController();
  const ctx = { signal: c.signal, events: [], event() {} };
  setTimeout(() => ctx.events.push({ type: "released", id: "gate" }), 20);
  await action(ctx, {
    do: "wait.event",
    type: "released",
    id: "gate",
    timeout_ms: 500,
  });
});
test("abort cancels delays", async () => {
  const c = new AbortController();
  const ctx = { signal: c.signal, event() {} };
  setTimeout(() => c.abort(), 20);
  await assert.rejects(action(ctx, { do: "delay", ms: 1000 }));
});
