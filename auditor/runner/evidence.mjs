import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
export const root = path.resolve(import.meta.dirname, "../..");
export const auditRoot = path.join(root, "auditor");
export class Blocked extends Error {}
export function inside(base, p) {
  const b = fs.realpathSync(base);
  const dest = path.resolve(b, p);
  if (dest !== b && !dest.startsWith(b + path.sep))
    throw Error("Path escapes allowed root");
  let cursor = dest;
  while (!fs.existsSync(cursor)) cursor = path.dirname(cursor);
  const real = fs.realpathSync(cursor);
  if (real !== b && !real.startsWith(b + path.sep))
    throw Error("Symlink escapes allowed root");
  return dest;
}
export function safeId(id) {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9_-]{0,79}$/.test(id))
    throw Error("Unsafe identifier");
  return id;
}
export function redactor(secrets) {
  return (value) => {
    let text = typeof value === "string" ? value : JSON.stringify(value);
    for (const s of [...secrets].sort((a, b) => b.length - a.length))
      if (s.length >= 4) text = text.split(s).join("[REDACTED]");
    return text.replace(/Bearer\s+[A-Za-z0-9._-]+/g, "Bearer [REDACTED]");
  };
}
export function sha(file) {
  return crypto
    .createHash("sha256")
    .update(fs.readFileSync(file))
    .digest("hex");
}
export function save(file, data, secrets = []) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  fs.writeFileSync(
    file,
    redactor(secrets)(JSON.stringify(data, null, 2)) + "\n",
    { mode: 0o600 },
  );
}

export function artifactPath(relative) {
  if (typeof relative !== "string" || path.isAbsolute(relative))
    throw Error("Artifact path must be relative");
  const runs = path.join(auditRoot, ".runs");
  if (fs.existsSync(runs) && fs.lstatSync(runs).isSymbolicLink())
    throw Error("Run root must not be a symlink");
  const dest = path.resolve(auditRoot, relative);
  if (!dest.startsWith(runs + path.sep))
    throw Error("Artifacts must stay under auditor/.runs");
  fs.mkdirSync(runs, { recursive: true, mode: 0o700 });
  return inside(runs, path.relative(runs, dest));
}
