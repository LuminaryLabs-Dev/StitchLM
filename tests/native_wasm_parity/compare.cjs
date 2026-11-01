// Real-weight test: Node hosts the WASM module, it does not substitute model work.
const fs=require('node:fs');const assert=require('node:assert/strict');
const [pkg,dir]=process.argv.slice(2);if(!pkg||!dir)throw Error('WASM Node package and real parity corpus required');
const {WebConnector}=require(pkg);
let max=0,steps=0;
for(const start of [0,2]){
 const worker=new WebConnector(fs.readFileSync(`${dir}/assignment-${start}.json`,'utf8'),fs.readFileSync(`${dir}/shard-${start}.safetensors`));
 const index=start/2;
 for(let step=0;fs.existsSync(`${dir}/job-${step}-${index}.json`);step++){
  const job=fs.readFileSync(`${dir}/job-${step}-${index}.json`,'utf8');const actual=JSON.parse(worker.execute(job));
  const reference=JSON.parse(fs.readFileSync(`${dir}/result-${step}-${index}.json`,'utf8'));
  assert.deepEqual(actual.output.shape,reference.output.shape);assert.equal(actual.output.values.length,reference.output.values.length);
  for(let i=0;i<actual.output.values.length;i++){assert(Number.isFinite(actual.output.values[i]));max=Math.max(max,Math.abs(actual.output.values[i]-reference.output.values[i]));}
  assert.deepEqual(JSON.parse(worker.execute(job)),actual);steps++;
 }
 worker.free();
}
assert(max<0.002,`native/WASM maximum activation error ${max}`);
console.log(JSON.stringify({synthetic:false,runtime:'Node-hosted actual Rust WASM, not browser UI',steps,maximum_activation_error:max,tolerance:0.002,duplicate_idempotency:true}));
