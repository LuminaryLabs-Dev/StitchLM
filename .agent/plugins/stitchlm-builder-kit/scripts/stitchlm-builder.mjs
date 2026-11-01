#!/usr/bin/env node
import {readFile, writeFile, mkdir, readdir, open, unlink, realpath, lstat, rename} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {spawnSync} from 'node:child_process';

const plugin = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const STAGES = Object.freeze(['Stage1', 'Stage2', 'Stage3']);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const encode = value => JSON.stringify(value, null, 2) + '\n';
const need = (condition, message) => { if (!condition) throw new Error(message); };
const json = async file => JSON.parse(await readFile(file, 'utf8'));
const text = value => typeof value === 'string' && value.trim().length > 0;
const sha = value => /^[a-f0-9]{40}$/.test(value ?? '');

export function validateRequest(r) {
  need(r?.schema === 'stitchlm-build-request/1', 'Unsupported request schema');
  for (const key of ['outcome', 'intent', 'tracker', 'authorization']) need(text(r[key]), `Missing ${key}`);
  need(r.repository === 'LuminaryLabs-Dev/StitchLM' && sha(r.commit), 'Exact repository and full commit required');
  for (const key of ['acceptance', 'exclusions', 'allowedPaths']) need(Array.isArray(r[key]) && r[key].length > 0 && r[key].every(text), `Explicit ${key} required`);
  need(r.allowedPaths.every(p => !path.isAbsolute(p) && !p.split('/').includes('..') && p !== '.'), 'Allowed paths must be explicit relative targets');
  need(Array.isArray(r.pins) && r.pins.length > 0 && r.pins.every(e => text(e.path) && !path.isAbsolute(e.path) && !e.path.split('/').includes('..') && sha(e.sha)), 'Pinned relative source files required');
  need(Array.isArray(r.requiredGates) && r.requiredGates.includes('source') && r.requiredGates.every(g => ['source', 'software', 'numerical', 'model', 'hardware', 'benchmark'].includes(g)), 'Explicit proof gates required');
  need(typeof r.fixture === 'boolean' && r.publish === false, 'Declare fixture; publication must be false');
  need(Number.isInteger(r.maxAttempts) && r.maxAttempts > 0 && r.maxAttempts <= 100, 'Iteration budget 1..100 required');
  if (r.integration) {
    need(text(r.integration.authorization) && path.isAbsolute(r.integration.targetRoot), 'Explicit integration authorization and absolute targetRoot required');
    need(r.integration.expected && Object.keys(r.integration.expected).length > 0 && Object.entries(r.integration.expected).every(([p, value]) => text(p) && !path.isAbsolute(p) && !p.split('/').includes('..') && (value === null || sha(value))), 'Integration requires exact expected prior blobs (null means absent)');
  }
  return r;
}

export async function verifyPlugin() {
  const portable = await json(path.join(plugin, 'plugin.json'));
  const compat = await json(path.join(plugin, '.codex-plugin/plugin.json'));
  need(portable.name === 'stitchlm-builder-kit' && compat.name === portable.name && compat.version === portable.version && compat.skills === './skills/', 'Plugin manifests do not agree');
  for (const name of ['load', 'select', 'inspect', 'define', 'build', 'validate', 'delivery']) {
    const body = await readFile(path.join(plugin, 'skills', name, 'SKILL.md'), 'utf8');
    need(body.startsWith(`---\nname: stitchlm-${name}\n`), `Plugin skill failed to load: ${name}`);
  }
  return {name: portable.name, version: portable.version, skills: 7, scope: 'local-package-preflight-not-model-session'};
}

// Source bytes, not an invented git HEAD, establish the caller-approved pin.
export async function verifySource(root, request) {
  root = await realpath(root);
  for (const entry of request.pins) {
    const file = path.join(root, entry.path);
    need((await realpath(file)).startsWith(root + path.sep), 'Source symlink escapes repository');
    const bytes = await readFile(file);
    const actual = createHash('sha1').update(Buffer.from(`blob ${bytes.length}\0`)).update(bytes).digest('hex');
    need(actual === entry.sha, `Pinned source changed: ${entry.path}`);
  }
  return {root, repository: request.repository, commit: request.commit, files: request.pins.length, scope: 'source-integrity-not-inference'};
}

async function packageDigest(root) {
  const entries = await collect(root, root);
  return hash(encode(entries));
}
export async function installationReceipt(codexHome, installedPath, executable = 'codex') {
  await verifyPlugin();
  const result = spawnSync(executable, ['plugin', 'list', '--json'], {encoding: 'utf8', env: {...process.env, CODEX_HOME: codexHome}, timeout: 30000, maxBuffer: 8 * 1024 * 1024});
  need(!result.error && result.status === 0, 'Native plugin list failed');
  const listed = JSON.parse(result.stdout).installed?.find(p => p.pluginId === 'stitchlm-builder-kit@stitchlm');
  need(listed?.installed === true && listed.enabled === true, 'Native plugin is not installed and enabled');
  const home = await realpath(codexHome), installed = await realpath(installedPath);
  need(installed === path.join(home, 'plugins', 'cache', 'stitchlm', 'stitchlm-builder-kit', (await json(path.join(plugin, 'plugin.json'))).version), 'Installed path must match exact native cache identity/version');
  const manifest = await json(path.join(installed, 'plugin.json'));
  need(manifest.name === 'stitchlm-builder-kit' && manifest.version === (await json(path.join(plugin, 'plugin.json'))).version, 'Installed package identity/version mismatch');
  const packageHash = await packageDigest(plugin);
  need(await packageDigest(installed) === packageHash, 'Installed package bytes differ; reinstall before real work');
  return {schema: 'stitchlm-installation/1', pluginId: listed.pluginId, version: manifest.version, packageHash, installedPath: installed, codexHome: home, executable, installed: true, enabled: true, modelSessionInvoked: false};
}
async function requireInstallation(request, receipt) {
  if (request.fixture) return; // Synthetic harness only, never real work or external proof.
  need(receipt?.schema === 'stitchlm-installation/1', 'Real work requires --installation native installed/enabled receipt');
  const current = await installationReceipt(receipt.codexHome, receipt.installedPath, receipt.executable);
  need(current.packageHash === receipt.packageHash && current.version === receipt.version, 'Installation receipt is stale');
}

async function load(run) {
  const names = (await readdir(run)).filter(n => /^\d{6}\.json$/.test(n)).sort();
  need(names.length > 0, 'Run not initialized');
  let previous = null, state;
  for (let i = 0; i < names.length; i++) {
    need(names[i] === `${String(i).padStart(6, '0')}.json`, 'Checkpoint gap');
    const bytes = await readFile(path.join(run, names[i]));
    const current = JSON.parse(bytes);
    need(current.previous === previous, 'Checkpoint hash chain changed');
    if (state) need(current.requestHash === state.requestHash, 'Target changed in checkpoint');
    need(hash(encode(current.request)) === current.requestHash, 'Request identity changed');
    validateRequest(current.request);
    need(STAGES.includes(current.stage), 'Invalid checkpoint stage');
    previous = hash(bytes); state = current;
  }
  return {state, previous, sequence: names.length};
}

async function append(run, loaded, changes) {
  const state = {...loaded.state, ...changes, previous: loaded.previous};
  await writeFile(path.join(run, `${String(loaded.sequence).padStart(6, '0')}.json`), encode(state), {flag: 'wx'});
  return state;
}

export async function initialize(run, request, installation) {
  validateRequest(request);
  await verifyPlugin();
  await requireInstallation(request, installation);
  await mkdir(path.dirname(path.resolve(run)), {recursive: true});
  await mkdir(run); // Never reuse or overwrite another worker's target.
  const state = {schema: 'stitchlm-build-state/1', request, requestHash: hash(encode(request)), stage: 'Stage1', status: 'READY', attempt: 0, history: [], artifacts: [], previous: null};
  await writeFile(path.join(run, '000000.json'), encode(state), {flag: 'wx'});
  return state;
}

async function verifyArtifacts(run, artifacts) {
  const base = await realpath(run);
  for (const artifact of artifacts) {
    const file = await realpath(path.resolve(base, artifact.path));
    need(file.startsWith(base + path.sep), 'Artifact escaped run directory');
    need(hash(await readFile(file)) === artifact.sha256, `Artifact changed: ${artifact.path}`);
  }
}

async function collect(run, directory) {
  const result = [];
  for (const name of (await readdir(directory)).sort()) {
    const file = path.join(directory, name), info = await lstat(file);
    need(!info.isSymbolicLink(), 'Worker artifact symlink rejected');
    if (info.isDirectory()) result.push(...await collect(run, file));
    else result.push({path: path.relative(run, file), sha256: hash(await readFile(file))});
  }
  return result;
}

function execute(command, cwd, input, timeoutMs = 120000) {
  need(Array.isArray(command) && command.length > 0 && command.every(text), 'Command must be an argument array');
  const result = spawnSync(command[0], command.slice(1), {cwd, input: input === undefined ? undefined : encode(input), encoding: 'utf8', timeout: timeoutMs, maxBuffer: 8 * 1024 * 1024, shell: false});
  return {command, status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout ?? '', stderr: result.stderr ?? ''};
}

export async function resume(run, options) {
  await verifyPlugin();
  run = await realpath(run);
  const lockPath = path.join(run, '.lock');
  const lock = await open(lockPath, 'wx').catch(error => { throw new Error(`Run locked; inspect owner before manual recovery (${error.code})`); });
  try {
    await lock.writeFile(encode({pid: process.pid, started: new Date().toISOString()}));
    const loaded = await load(run), s = loaded.state;
    await requireInstallation(s.request, options.installation);
    if (options.request) need(hash(encode(validateRequest(options.request))) === s.requestHash, 'Different outcome, intent or pin; refusing to switch target');
    if (s.status === 'COMPLETE') {await verifySource(options.root, s.request); await verifyArtifacts(run, s.artifacts); return s;}
    if (s.attempt >= s.request.maxAttempts) return append(run, loaded, {status: 'BLOCKED', reason: 'Iteration budget exhausted; review and authorize a new run'});
    const attempt = s.attempt + 1;
    const attemptDir = path.join(run, `attempt-${String(attempt).padStart(6, '0')}`);
    // An interrupted attempt is preserved; never silently repeat its external effects.
    try {await mkdir(attemptDir);} catch(error) {throw new Error(`Interrupted attempt exists; inspect ${attemptDir} before recovery: ${error.code}`);}
    const base = {attempt, stage: s.stage, status: 'BLOCKED', reason: null};
    const finish = async changes => append(run, loaded, {...base, ...changes, history: [...s.history, {attempt, from: s.stage, to: changes.stage ?? s.stage, status: changes.status ?? 'BLOCKED', reason: changes.reason ?? null}]});
    let source;
    try { source = await verifySource(options.root, s.request); await verifyArtifacts(run, s.artifacts); }
    catch(error) { return finish({stage: 'Stage1', reason: error.message, artifacts: [], plan: null, candidate: null}); }
    if (s.stage === 'Stage2' && !s.plan) return finish({stage: 'Stage1', status: 'RETRY', reason: 'Build requires an accepted Stage1 plan', artifacts: [], candidate: null});
    const worker = options.worker;
    if (!worker) return finish({reason: 'No authorized worker configured; supply --worker <config.json>'});
    need(worker.schema === 'stitchlm-worker/1', 'Unsupported worker configuration');
    need(worker.timeoutMs === undefined || (Number.isInteger(worker.timeoutMs) && worker.timeoutMs > 0 && worker.timeoutMs <= 300000), 'Worker timeout must be 1..300000 milliseconds');
    need(typeof worker.fixture === 'boolean' && worker.fixture === s.request.fixture, 'Fixture/live worker mismatch');
    const task = {schema: 'stitchlm-worker-task/1', stage: s.stage, request: s.request, requestHash: s.requestHash, attempt, outputDirectory: attemptDir, plan: s.plan ?? null, candidate: s.candidate ?? null, artifacts: s.artifacts, constraints: {publish: false, repositoryWrites: false, trackerWrites: false, sourceReadOnly: true}};
    const invocation = execute(worker.command, attemptDir, task, worker.timeoutMs ?? 120000);
    await writeFile(path.join(attemptDir, 'worker-log.json'), encode(invocation), {flag: 'wx'});
    if (invocation.error || invocation.status !== 0) return finish({reason: `Worker unavailable or failed; inspect attempt-${String(attempt).padStart(6, '0')}/worker-log.json`});
    try {await verifySource(options.root, s.request); await verifyArtifacts(run, s.artifacts);}
    catch(error) {return finish({stage: 'Stage1', status: 'RETRY', reason: error.message, artifacts: [], plan: null, candidate: null});}
    let result;
    try {
      result = JSON.parse(invocation.stdout);
      need(result.schema === 'stitchlm-worker-result/1' && result.requestHash === s.requestHash && result.stage === s.stage && result.attempt === attempt, 'Worker result identity mismatch');
      need(['PASS', 'ASSUMPTION', 'DEFECT', 'BLOCKED'].includes(result.status), 'Invalid worker result status');
      need(text(result.summary), 'Worker summary required');
    } catch(error) {return finish({reason: `Invalid worker result: ${error.message}`});}
    if (result.status === 'DEFECT' && !s.plan) result.status = 'ASSUMPTION';
    if (result.status !== 'PASS') {
      const stage = result.status === 'ASSUMPTION' ? 'Stage1' : result.status === 'DEFECT' ? 'Stage2' : s.stage;
      return finish({stage, status: result.status === 'BLOCKED' ? 'BLOCKED' : 'RETRY', reason: result.summary, ...(stage === 'Stage1' ? {artifacts: [], plan: null, candidate: null} : stage === 'Stage2' ? {artifacts: s.planArtifacts ?? [], candidate: null} : {})});
    }
    if (s.stage === 'Stage1') {
      const plan = result.plan;
      if (!plan || plan.intent !== s.request.intent || JSON.stringify(plan.acceptance) !== JSON.stringify(s.request.acceptance) || JSON.stringify(plan.exclusions) !== JSON.stringify(s.request.exclusions) || !text(plan.approach)) return finish({stage: 'Stage1', status: 'RETRY', reason: 'Plan must preserve exact intent, acceptance and exclusions'});
      await writeFile(path.join(attemptDir, 'plan.json'), encode(plan), {flag: 'wx'});
      return finish({stage: 'Stage2', status: 'READY', plan, source, planArtifacts: await collect(run, attemptDir), artifacts: await collect(run, attemptDir)});
    }
    if (s.stage === 'Stage2') {
      const candidate = result.candidate;
      if (!candidate || !text(candidate.summary) || !Array.isArray(candidate.files) || candidate.files.length === 0) return finish({stage: 'Stage2', status: 'RETRY', reason: 'Candidate files required'});
      for (const relative of candidate.files) {
        need(text(relative) && !path.isAbsolute(relative) && !relative.split('/').includes('..'), 'Candidate path must be relative');
        need(s.request.allowedPaths.some(p => relative === p || (p.endsWith('/') && relative.startsWith(p))), 'Candidate outside authorized target paths');
        const file = await realpath(path.join(attemptDir, relative));
        need(file.startsWith(attemptDir + path.sep), 'Candidate escaped attempt directory');
        await readFile(file);
      }
      return finish({stage: 'Stage3', status: 'READY', candidate: {...candidate, directory: path.relative(run, attemptDir)}, source, artifacts: [...s.artifacts, ...await collect(run, attemptDir)]});
    }
    if (!s.plan || !s.candidate) return finish({stage: 'Stage1', status: 'RETRY', reason: 'Validation requires accepted plan and candidate', artifacts: [], plan: null, candidate: null});
    const proof = {source, commands: [], fixture: s.request.fixture, inferenceProven: false, hardwareProven: false};
    // Acceptance commands are explicitly authorized local software checks, not arbitrary
    // shell strings or a substitute for browser/device evidence.
    for (const command of worker.softwareChecks ?? []) {
      const receipt = execute(command, run, {request: s.request, candidate: s.candidate, artifacts: s.artifacts}, worker.timeoutMs ?? 120000);
      proof.commands.push(receipt);
      if (receipt.error) {await writeFile(path.join(attemptDir, 'proof.json'), encode(proof)); return finish({reason: receipt.error});}
      if (receipt.status !== 0) {await writeFile(path.join(attemptDir, 'proof.json'), encode(proof)); return finish({stage: 'Stage2', status: 'RETRY', reason: 'Software acceptance command failed', artifacts: s.planArtifacts ?? [], candidate: null});}
    }
    // Recheck after worker execution: neither workers nor checks may mutate pinned source.
    try {await verifySource(options.root, s.request); await verifyArtifacts(run, s.artifacts);}
    catch(error) {return finish({stage: 'Stage1', status: 'RETRY', reason: error.message, artifacts: [], plan: null, candidate: null});}
    await writeFile(path.join(attemptDir, 'proof.json'), encode(proof), {flag: 'wx'});
    const missing = [];
    if (s.request.requiredGates.includes('software') && proof.commands.length === 0) missing.push('software acceptance commands');
    proof.external = [];
    const candidateHash = hash(encode({candidate: s.candidate, artifacts: s.artifacts}));
    for (const gate of s.request.requiredGates.filter(g => !['source', 'software'].includes(g))) {
      const check = worker.evidenceChecks?.[gate];
      if (s.request.fixture || !check) {missing.push(`${gate}: ${s.request.fixture ? 'fixture cannot establish external proof' : 'no authorized evidence command'}`); continue;}
      const receipt = execute(check, attemptDir, {schema: 'stitchlm-evidence-task/1', gate, request: s.request, requestHash: s.requestHash, candidateHash, candidate: s.candidate, artifacts: s.artifacts, runDirectory: run, outputDirectory: attemptDir}, worker.timeoutMs ?? 120000);
      await writeFile(path.join(attemptDir, `evidence-${gate}-command.json`), encode(receipt), {flag: 'wx'});
      if (receipt.error || receipt.status !== 0) {missing.push(`${gate}: command unavailable or failed; inspect raw receipt`); continue;}
      let evidence;
      try {
        evidence = JSON.parse(receipt.stdout);
        need(evidence.schema === 'stitchlm-evidence-result/1' && evidence.gate === gate && evidence.requestHash === s.requestHash && evidence.candidateHash === candidateHash && evidence.fixture === false, 'Evidence identity/scope mismatch');
        need(evidence.passed === true && text(evidence.procedure) && evidence.environment && Object.keys(evidence.environment).length > 0 && evidence.measurements && Object.keys(evidence.measurements).length > 0 && Array.isArray(evidence.limitations), 'Actual procedure, environment, measurements, limitations and passing result required');
        need(Array.isArray(evidence.files) && evidence.files.length > 0, 'Raw evidence files required');
        for (const relative of evidence.files) {
          need(text(relative) && !path.isAbsolute(relative) && !relative.split('/').includes('..'), 'Evidence file must be relative');
          const file = await realpath(path.join(attemptDir, relative));
          need(file.startsWith(attemptDir + path.sep) && (await readFile(file)).length > 0, 'Raw evidence must exist within current attempt');
        }
        proof.external.push({gate, evidence, command: receipt.command});
      } catch(error) {missing.push(`${gate}: ${error.message}`);}
    }
    try {await verifySource(options.root, s.request); await verifyArtifacts(run, s.artifacts);}
    catch(error) {return finish({stage: 'Stage1', status: 'RETRY', reason: error.message, artifacts: [], plan: null, candidate: null});}
    proof.inferenceProven = proof.external.some(e => e.gate === 'model');
    proof.hardwareProven = proof.external.some(e => e.gate === 'hardware');
    await writeFile(path.join(attemptDir, 'proof.json'), encode(proof));
    const artifacts = [...s.artifacts, ...await collect(run, attemptDir)];
    if (missing.length) return finish({status: 'BLOCKED', reason: `External evidence required: ${missing.join(', ')}; no inferred passes`, artifacts});
    const delivery = {schema: 'stitchlm-delivery/1', request: s.request, requestHash: s.requestHash, proof, artifacts, status: s.request.fixture ? 'FIXTURE_ONLY' : proof.external.length ? 'SCOPED_EVIDENCE_ONLY' : proof.commands.length ? 'SOURCE_SOFTWARE_ONLY' : 'SOURCE_ONLY', published: false, repositoryModified: false, trackerModified: false, limitations: ['Only explicitly recorded proof gates are supported; no automatic scaling or release proof', 'Candidate handoff is not an implemented distributed inference system', 'Configured worker is trusted, not OS-sandboxed by this CLI']};
    await writeFile(path.join(attemptDir, 'delivery.json'), encode(delivery), {flag: 'wx'});
    return finish({status: 'COMPLETE', reason: delivery.status, artifacts: [...s.artifacts, ...await collect(run, attemptDir)]});
  } finally {await lock.close(); await unlink(lockPath);}
}

// Recovery requires a human reason and refuses a live owner. It never reruns a worker.
export async function recover(run, reason) {
  need(text(reason), 'Recovery reason required');
  run = await realpath(run);
  const lockPath = path.join(run, '.lock');
  let owner;
  try {owner = await json(lockPath);} catch(error) {if (error.code !== 'ENOENT') throw error;}
  if (owner) {
    need(Number.isInteger(owner.pid) && owner.pid > 0, 'Invalid lock owner; inspect manually');
    let alive = true;
    try {process.kill(owner.pid, 0);} catch(error) {if (error.code === 'ESRCH') alive = false; else throw error;}
    need(!alive, 'Lock owner is alive; recovery refused');
    await unlink(lockPath);
  }
  const lock = await open(lockPath, 'wx');
  try {
    await lock.writeFile(encode({pid: process.pid, recovery: true}));
    const loaded = await load(run);
    const attempts = (await readdir(run)).filter(n => /^attempt-\d{6}$/.test(n)).map(n => Number(n.slice(8)));
    return append(run, loaded, {status: 'BLOCKED', reason: `Interrupted attempt reviewed: ${reason}`, attempt: Math.max(loaded.state.attempt, ...attempts), history: [...loaded.state.history, {recovery: reason, from: loaded.state.stage, to: loaded.state.stage}]});
  } finally {await lock.close(); await unlink(lockPath);}
}

const blob = bytes => createHash('sha1').update(Buffer.from(`blob ${bytes.length}\0`)).update(bytes).digest('hex');
export async function integrate(run, installation) {
  await verifyPlugin();
  run = await realpath(run);
  const lockPath = path.join(run, '.lock'), lock = await open(lockPath, 'wx');
  try {
    await lock.writeFile(encode({pid: process.pid, integration: true}));
    const loaded = await load(run), s = loaded.state, contract = s.request.integration;
    await requireInstallation(s.request, installation);
    need(s.status === 'COMPLETE' && contract, 'Integration requires COMPLETE and explicit request-pinned integration authorization');
    need(!s.request.fixture, 'Fixture cannot integrate into a real worktree');
    await verifyArtifacts(run, s.artifacts);
    const root = await realpath(contract.targetRoot), operations = [];
    for (const relative of s.candidate.files) {
      need(Object.hasOwn(contract.expected, relative), `Missing integration precondition: ${relative}`);
      need(s.request.allowedPaths.some(p => relative === p || (p.endsWith('/') && relative.startsWith(p))), 'Integration target outside scope');
      const target = path.join(root, relative);
      // No directory creation or symlink traversal: caller provisions authorized worktree first.
      const parent = await realpath(path.dirname(target));
      need(parent === root || parent.startsWith(root + path.sep), 'Integration parent escaped target');
      let current = null;
      try {need(!(await lstat(target)).isSymbolicLink(), 'Integration symlink rejected'); current = blob(await readFile(target));}
      catch(error) {if (error.code !== 'ENOENT') throw error;}
      const bytes = await readFile(path.join(run, s.candidate.directory, relative)), desired = blob(bytes);
      need(current === contract.expected[relative] || current === desired, `Integration conflict: ${relative}`);
      operations.push({relative, target, bytes, desired, current});
    }
    // Revalidate source dependencies before integration, including after a delay since validation.
    // Candidate targets themselves are guarded by exact before/after preconditions above.
    const sourceRoot = await realpath(s.source.root);
    await verifySource(sourceRoot, {...s.request, pins: sourceRoot === root ? s.request.pins.filter(p => !s.candidate.files.includes(p.path)) : s.request.pins});
    // Journal before effects; reruns accept only prior or exact desired bytes, never unrelated edits.
    const journal = path.join(run, 'integration-journal.json');
    try {await writeFile(journal, encode({requestHash: s.requestHash, targetRoot: root, files: operations.map(o => ({path: o.relative, before: contract.expected[o.relative], after: o.desired}))}), {flag: 'wx'});}
    catch(error) {if (error.code !== 'EEXIST') throw error; need((await json(journal)).requestHash === s.requestHash, 'Integration journal identity mismatch');}
    for (const op of operations) {
      if (op.current === op.desired) continue;
      // Exclusive temp file and rename keep each file atomic. Multi-file interruption is resumable, not transactional.
      const temporary = `${op.target}.stitchlm-${s.requestHash.slice(0, 12)}.tmp`;
      let latest = null;
      try {need(!(await lstat(op.target)).isSymbolicLink(), 'Integration symlink appeared'); latest = blob(await readFile(op.target));} catch(error) {if (error.code !== 'ENOENT') throw error;}
      need(latest === op.current, `Concurrent integration edit: ${op.relative}`);
      try {await writeFile(temporary, op.bytes, {flag: 'wx'});}
      catch(error) {
        if (error.code !== 'EEXIST') throw error;
        need(!(await lstat(temporary)).isSymbolicLink(), 'Interrupted integration temp is a symlink');
        need(blob(await readFile(temporary)) === op.desired, 'Interrupted integration temp bytes differ; inspect manually');
      }
      // Recheck after preparing/reusing a temp; never replace a changed destination.
      let beforeRename = null;
      try {need(!(await lstat(op.target)).isSymbolicLink(), 'Integration symlink appeared'); beforeRename = blob(await readFile(op.target));} catch(error) {if (error.code !== 'ENOENT') throw error;}
      need(beforeRename === op.current, `Concurrent integration edit: ${op.relative}`);
      await rename(temporary, op.target);
      need(blob(await readFile(op.target)) === op.desired, 'Integration readback failed');
    }
    return append(run, loaded, {integration: {targetRoot: root, files: operations.map(o => ({path: o.relative, sha: o.desired})), published: false}, reason: 'Candidate integrated under explicit scope; not published'});
  } finally {await lock.close(); await unlink(lockPath);}
}

export async function inspect(run) {const {state} = await load(run); await verifyArtifacts(run, state.artifacts); return state;}

async function main() {
  const [command, ...args] = process.argv.slice(2);
  const flags = {};
  for (let i = 0; i < args.length; i += 2) {need(args[i]?.startsWith('--') && args[i+1], 'Use --key value'); need(!Object.hasOwn(flags, args[i]), 'Duplicate option'); flags[args[i]] = args[i+1];}
  if (command === 'verify-install') {
    const receipt = await installationReceipt(flags['--codex-home'], flags['--installed-path'], flags['--codex'] ?? 'codex');
    need(flags['--receipt'], 'Provide --receipt output path');
    await writeFile(flags['--receipt'], encode(receipt));
    console.log(encode(receipt)); return;
  }
  const installation = flags['--installation'] ? await json(flags['--installation']) : undefined;
  need(flags['--run'], 'Usage: stitchlm-builder.mjs init|resume|inspect|recover|integrate --run <new-directory> [--request request.json] [--root StitchLM] [--worker worker.json]');
  let result;
  if (command === 'integrate') result = await integrate(flags['--run'], installation);
  else if (command === 'init') result = await initialize(flags['--run'], await json(flags['--request']), installation);
  else if (command === 'recover') result = await recover(flags['--run'], flags['--reason']);
  else if (command === 'inspect') result = await inspect(flags['--run']);
  else if (command === 'resume') result = await resume(flags['--run'], {installation, root: path.resolve(flags['--root'] ?? '.'), worker: flags['--worker'] ? await json(flags['--worker']) : undefined, request: flags['--request'] ? await json(flags['--request']) : undefined});
  else throw new Error('Unknown command; use init, resume, inspect, recover or integrate');
  console.log(encode(result));
  if (result.status === 'BLOCKED') process.exitCode = 2;
  else if (result.status === 'RETRY') process.exitCode = 3;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => {console.error(encode({status: 'BLOCKED', error: error.message})); process.exitCode = 2;});
