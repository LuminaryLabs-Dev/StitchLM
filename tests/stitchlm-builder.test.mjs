import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, writeFile, readFile, mkdir, rm, symlink} from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {initialize, resume, inspect, recover, validateRequest, integrate, verifyPlugin} from '../.agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs';
const installation = process.env.STITCHLM_TEST_INSTALLATION ? JSON.parse(await readFile(process.env.STITCHLM_TEST_INSTALLATION, 'utf8')) : undefined;
const nativeTest = (name, fn) => test(name, {skip: !installation && 'Requires verified native installation receipt'}, fn);
const script = new URL('../.agent/plugins/stitchlm-builder-kit/scripts/stitchlm-builder.mjs', import.meta.url);
async function setup(t, overrides={}) {
 const dir=await mkdtemp(path.join(os.tmpdir(),'stitchlm-test-')); t.after(()=>rm(dir,{recursive:true,force:true}));
 const root=path.join(dir,'source'); await mkdir(root); await writeFile(path.join(root,'README.md'),'Synthetic source\n');
 const bytes=await readFile(path.join(root,'README.md')); const sha=createHash('sha1').update(Buffer.from(`blob ${bytes.length}\0`)).update(bytes).digest('hex');
 const request={schema:'stitchlm-build-request/1',repository:'LuminaryLabs-Dev/StitchLM',commit:'a'.repeat(40),outcome:'Synthetic candidate',intent:'Prove harness orchestration only',tracker:'test fixture, not a real task',authorization:'synthetic local test only',acceptance:['candidate contains expected bytes'],exclusions:['No inference or external effects'],allowedPaths:['fixture.txt'],pins:[{path:'README.md',sha}],requiredGates:['source','software'],fixture:true,publish:false,maxAttempts:10,...overrides};
 const workerPath=path.join(dir,'worker.mjs');
 await writeFile(workerPath,`import {writeFileSync,readFileSync} from 'node:fs';
 const t=JSON.parse(readFileSync(0,'utf8')); const mode=process.argv[2]||'PASS';
 const r={schema:'stitchlm-worker-result/1',stage:t.stage,attempt:t.attempt,requestHash:t.requestHash,status:mode,summary:'Synthetic fixture only'};
 if(t.stage==='Stage1')r.plan={intent:t.request.intent,acceptance:t.request.acceptance,exclusions:t.request.exclusions,approach:'Write and independently verify a fixture'};
 if(t.stage==='Stage2'){writeFileSync('fixture.txt','synthetic candidate\\n');r.candidate={summary:'synthetic',files:['fixture.txt']};}
 if(mode==='WRONG'){r.status='PASS';r.requestHash='wrong';}
 if(mode==='DRIFT'){r.status='PASS';r.plan.intent='changed';}
 console.log(JSON.stringify(r));`);
 const check=path.join(dir,'check.mjs'); await writeFile(check,`import {readFileSync} from 'node:fs';import assert from 'node:assert/strict';const t=JSON.parse(readFileSync(0,'utf8'));const a=t.artifacts.find(a=>a.path.endsWith('/fixture.txt'));assert.ok(a);assert.equal(readFileSync(a.path,'utf8'),'synthetic candidate\\n');`);
 const worker={schema:'stitchlm-worker/1',fixture:true,command:[process.execPath,workerPath],softwareChecks:[[process.execPath,check]]};
 const run=path.join(dir,'run'); await initialize(run,request,installation); const opts={root,worker,installation};
 return {dir,root,request,worker,run,opts,step:()=>resume(run,opts),mode:m=>{worker.command=[process.execPath,workerPath,m];}};
}
test('three-stage resume preserves intent and independently validates fixture bytes',async t=>{const x=await setup(t);assert.equal((await x.step()).stage,'Stage2');assert.equal((await x.step()).stage,'Stage3');const s=await x.step();assert.equal(s.status,'COMPLETE');assert.equal(s.reason,'FIXTURE_ONLY');assert.deepEqual(s.request,x.request);assert.equal((await x.step()).attempt,3);assert.equal((await inspect(x.run)).status,'COMPLETE');});
test('no worker is blocked, not silently simulated',async t=>{const x=await setup(t);assert.equal((await resume(x.run,{root:x.root})).status,'BLOCKED');});
test('request identity drift rejected',async t=>{const x=await setup(t);await assert.rejects(resume(x.run,{...x.opts,request:{...x.request,intent:'different'}}),/Different outcome/);});
test('plan intent drift returns definition',async t=>{const x=await setup(t);x.mode('DRIFT');const s=await x.step();assert.equal(s.stage,'Stage1');assert.equal(s.status,'RETRY');});
test('worker identity mismatch cannot pass',async t=>{const x=await setup(t);x.mode('WRONG');assert.equal((await x.step()).status,'BLOCKED');});
test('failed assumptions return inspect/define and clear candidate',async t=>{const x=await setup(t);await x.step();await x.step();x.mode('ASSUMPTION');const s=await x.step();assert.equal(s.stage,'Stage1');assert.equal(s.plan,null);assert.equal(s.candidate,null);});
test('implementation defects return build',async t=>{const x=await setup(t);await x.step();await x.step();x.mode('DEFECT');const s=await x.step();assert.equal(s.stage,'Stage2');assert.ok(s.plan);assert.equal(s.candidate,null);});
test('defect before accepted plan returns definition',async t=>{const x=await setup(t);x.mode('DEFECT');assert.equal((await x.step()).stage,'Stage1');});
test('blocker holds stage',async t=>{const x=await setup(t);await x.step();x.mode('BLOCKED');const s=await x.step();assert.equal(s.stage,'Stage2');assert.equal(s.status,'BLOCKED');});
test('budget exhaustion never executes another worker',async t=>{const x=await setup(t,{maxAttempts:1});await x.step();const s=await x.step();assert.equal(s.status,'BLOCKED');assert.equal(s.attempt,1);assert.match(s.reason,/budget/);});
test('source drift invalidates evidence and returns definition',async t=>{const x=await setup(t);await x.step();await writeFile(path.join(x.root,'README.md'),'changed');const s=await x.step();assert.equal(s.stage,'Stage1');assert.equal(s.plan,null);});
test('artifact drift invalidates evidence',async t=>{const x=await setup(t);await x.step();await x.step();await writeFile(path.join(x.run,'attempt-000002/fixture.txt'),'changed');const s=await x.step();assert.equal(s.stage,'Stage1');assert.equal(s.status,'BLOCKED');});
test('complete state still checks evidence',async t=>{const x=await setup(t);await x.step();await x.step();await x.step();await writeFile(path.join(x.run,'attempt-000002/fixture.txt'),'changed');await assert.rejects(x.step(),/Artifact changed/);});
test('external model/numerical/hardware/benchmark gates cannot be asserted by fixture',async t=>{const x=await setup(t,{requiredGates:['source','software','model','numerical','hardware','benchmark']});await x.step();await x.step();const s=await x.step();assert.equal(s.status,'BLOCKED');assert.match(s.reason,/External evidence/);});
test('software check failure returns build',async t=>{const x=await setup(t);await x.step();await x.step();x.worker.softwareChecks=[[process.execPath,'-e','process.exit(1)']];const s=await x.step();assert.equal(s.stage,'Stage2');assert.equal(s.status,'RETRY');});
test('missing acceptance commands blocks software proof',async t=>{const x=await setup(t);await x.step();await x.step();x.worker.softwareChecks=[];assert.equal((await x.step()).status,'BLOCKED');});
test('fixture/live mismatch rejected',async t=>{const x=await setup(t);x.worker.fixture=false;await assert.rejects(x.step(),/mismatch/);});
test('lock refuses overlapping ownership and recovery of live process',async t=>{const x=await setup(t);await writeFile(path.join(x.run,'.lock'),JSON.stringify({pid:process.pid}));await assert.rejects(x.step(),/locked/);await assert.rejects(recover(x.run,'test'),/alive/);});
test('interrupted attempt requires deliberate recovery, not replay',async t=>{const x=await setup(t);await mkdir(path.join(x.run,'attempt-000001'));await assert.rejects(x.step(),/Interrupted attempt/);assert.equal((await recover(x.run,'Reviewed synthetic interrupted attempt')).attempt,1);assert.equal((await x.step()).attempt,2);});
test('checkpoint tampering detected',async t=>{const x=await setup(t);await x.step();const f=path.join(x.run,'000000.json');const s=JSON.parse(await readFile(f));s.request.intent='tampered';await writeFile(f,JSON.stringify(s));await assert.rejects(inspect(x.run),/identity|hash chain/);});
test('unsafe pins and auto-publication rejected',async t=>{const x=await setup(t);assert.throws(()=>validateRequest({...x.request,publish:true}));assert.throws(()=>validateRequest({...x.request,pins:[{path:'../outside',sha:'a'.repeat(40)}]}));});
test('source symlink escape rejected',async t=>{const x=await setup(t);await rm(path.join(x.root,'README.md'));await writeFile(path.join(x.dir,'outside'),'Synthetic source\n');await symlink(path.join(x.dir,'outside'),path.join(x.root,'README.md'));assert.match((await x.step()).reason,/symlink escapes/);});
test('manifest and explicit skill discovery agree',async()=>{const base=new URL('../.agent/plugins/stitchlm-builder-kit/',import.meta.url);const manifest=JSON.parse(await readFile(new URL('plugin.json',base)));const compat=JSON.parse(await readFile(new URL('.codex-plugin/plugin.json',base)));assert.equal(manifest.name,compat.name);assert.equal(compat.skills,'./skills/');for(const name of ['load','select','inspect','define','build','validate','delivery'])assert.match(await readFile(new URL(`skills/${name}/SKILL.md`,base),'utf8'),/^---\nname: stitchlm-/);const market=JSON.parse(await readFile(new URL('../.agents/plugins/marketplace.json',import.meta.url)));assert.equal(market.plugins[0].source.path,'./.agent/plugins/stitchlm-builder-kit');});
test('candidate outside allowed paths is rejected',async t=>{const x=await setup(t,{allowedPaths:['other.txt']});await x.step();await assert.rejects(x.step(),/authorized target/);});
test('plan evidence remains guarded after defect loops',async t=>{const x=await setup(t);await x.step();await x.step();x.mode('DEFECT');await x.step();await writeFile(path.join(x.run,'attempt-000001/plan.json'),'tampered');const s=await x.step();assert.equal(s.stage,'Stage1');assert.match(s.reason,/Artifact changed/);});
test('blocked attempts consume budget',async t=>{const x=await setup(t,{maxAttempts:1});x.mode('BLOCKED');await x.step();assert.match((await x.step()).reason,/budget/);});
test('duplicate initialize never overwrites run',async t=>{const x=await setup(t);await assert.rejects(initialize(x.run,x.request),/EEXIST/);assert.equal((await inspect(x.run)).attempt,0);});

test('package preflight loads all explicit skills',async()=>{assert.equal((await verifyPlugin()).skills,7);});
nativeTest('live-mode gate contract executes configured command and binds raw evidence (synthetic protocol test only)',async t=>{
 const x=await setup(t,{fixture:false,requiredGates:['source','software','numerical']});x.worker.fixture=false;
 const check=path.join(x.dir,'evidence.mjs');await writeFile(check,`import {readFileSync,writeFileSync} from 'node:fs';const t=JSON.parse(readFileSync(0,'utf8'));writeFileSync('raw-evidence.txt','Synthetic protocol fixture, not numerical inference');console.log(JSON.stringify({schema:'stitchlm-evidence-result/1',gate:t.gate,requestHash:t.requestHash,candidateHash:t.candidateHash,fixture:false,passed:true,procedure:'Synthetic contract exercise, not inference',environment:{test:true},measurements:{fixtureValue:1},limitations:['Synthetic protocol test only'],files:['raw-evidence.txt']}));`);
 x.worker.evidenceChecks={numerical:[process.execPath,check]};await x.step();await x.step();const s=await x.step();assert.equal(s.status,'COMPLETE');assert.equal(s.reason,'SCOPED_EVIDENCE_ONLY');
});
nativeTest('malformed or unbound evidence cannot satisfy live gates',async t=>{const x=await setup(t,{fixture:false,requiredGates:['source','numerical']});x.worker.fixture=false;x.worker.evidenceChecks={numerical:[process.execPath,'-e','console.log(JSON.stringify({passed:true}))']};await x.step();await x.step();assert.equal((await x.step()).status,'BLOCKED');});
nativeTest('missing live evidence command blocks instead of permanent gate refusal',async t=>{const x=await setup(t,{fixture:false,requiredGates:['source','hardware']});x.worker.fixture=false;await x.step();await x.step();assert.match((await x.step()).reason,/no authorized evidence command/);});
nativeTest('explicit integration safely copies into authorized temporary worktree and is idempotent',async t=>{
 const target=await mkdtemp(path.join(os.tmpdir(),'stitchlm-target-'));t.after(()=>rm(target,{recursive:true,force:true}));
 const x=await setup(t,{fixture:false,integration:{authorization:'Synthetic scoped copy test only',targetRoot:target,expected:{'fixture.txt':null}}});x.worker.fixture=false;await x.step();await x.step();await x.step();const s=await integrate(x.run,installation);assert.equal(s.integration.published,false);assert.equal(await readFile(path.join(target,'fixture.txt'),'utf8'),'synthetic candidate\n');await integrate(x.run,installation);
});
nativeTest('integration conflict preserves existing work',async t=>{
 const target=await mkdtemp(path.join(os.tmpdir(),'stitchlm-target-'));t.after(()=>rm(target,{recursive:true,force:true}));
 const x=await setup(t,{fixture:false,integration:{authorization:'Synthetic scoped copy test only',targetRoot:target,expected:{'fixture.txt':null}}});x.worker.fixture=false;await x.step();await x.step();await x.step();await writeFile(path.join(target,'fixture.txt'),'another worker');await assert.rejects(integrate(x.run,installation),/conflict/);assert.equal(await readFile(path.join(target,'fixture.txt'),'utf8'),'another worker');
});
test('integration is never automatic or available without explicit contract',async t=>{const x=await setup(t);await x.step();await x.step();await x.step();await assert.rejects(integrate(x.run,installation),/explicit/);});

test('real work without verified installation is blocked before initialization',async t=>{const x=await setup(t);await assert.rejects(initialize(path.join(x.dir,'real'),{...x.request,fixture:false}),/installation/);});

nativeTest('integration resumes exact interrupted temp without overwriting unrelated bytes',async t=>{
 const target=await mkdtemp(path.join(os.tmpdir(),'stitchlm-target-'));t.after(()=>rm(target,{recursive:true,force:true}));
 const x=await setup(t,{fixture:false,integration:{authorization:'Synthetic scoped copy test only',targetRoot:target,expected:{'fixture.txt':null}}});x.worker.fixture=false;await x.step();await x.step();const state=await x.step();
 const temp=path.join(target,`fixture.txt.stitchlm-${state.requestHash.slice(0,12)}.tmp`);await writeFile(temp,'unrelated bytes');await assert.rejects(integrate(x.run,installation),/temp bytes differ/);await assert.rejects(readFile(path.join(target,'fixture.txt')),/ENOENT/);
 await writeFile(temp,'synthetic candidate\n');await integrate(x.run,installation);assert.equal(await readFile(path.join(target,'fixture.txt'),'utf8'),'synthetic candidate\n');await assert.rejects(readFile(temp),/ENOENT/);
});
