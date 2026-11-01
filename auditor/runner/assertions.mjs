import assert from "node:assert/strict";
export function field(value, key) {
  for (const bit of key.split(".")) {
    if (["__proto__", "constructor", "prototype"].includes(bit))
      throw Error("Forbidden field");
    value = value?.[bit];
  }
  return value;
}
export function check(value, rule) {
  const actual = rule.field ? field(value, rule.field) : value;
  if ("equals" in rule) assert.deepEqual(actual, rule.equals);
  if ("contains" in rule)
    assert.ok(
      actual?.includes(rule.contains),
      `Expected ${rule.field} to contain ${rule.contains}`,
    );
  if ("gt" in rule)
    assert.ok(
      typeof actual === "number" && actual > rule.gt,
      `Expected ${rule.field} > ${rule.gt}`,
    );
  if ("lte" in rule)
    assert.ok(
      typeof actual === "number" && actual <= rule.lte,
      `Expected ${rule.field} <= ${rule.lte}`,
    );
}
export function generation(events, expect) {
  assert.ok(Array.isArray(events) && events.length, "Empty generation");
  const last = events.at(-1);
  for (const rule of expect) check(last, rule);
  return last;
}
