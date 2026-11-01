import init from './pkg/stitchlm_host_web.js';
init().catch(e=>document.getElementById('status').textContent=String(e));
