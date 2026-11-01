// Thin browser worker lifecycle glue. Model, protocol and network work are Rust/WASM.
let worker;
const status = document.getElementById('status');
document.getElementById('connect').onclick = () => {
 if (worker) return;
 worker = new Worker('./worker.js', {type:'module'});
 worker.onmessage = e => { status.textContent = String(e.data); };
 worker.onerror = e => { status.textContent = `Worker failed: ${e.message}`; worker.terminate(); worker = null; };
 worker.postMessage({pair:document.getElementById('pair').value,name:document.getElementById('name').value,memory:Number(document.getElementById('memory').value)});
 document.getElementById('pair').value = '';
 status.textContent = 'Loading WASM runtime…';
};
document.getElementById('disconnect').onclick = () => { worker?.terminate(); worker = null; status.textContent = 'Worker stopped. Remove its old assignment in the host before pairing again.'; };
