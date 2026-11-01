import init, {connect} from './pkg/stitchlm_connector_web.js';
self.onmessage = async ({data}) => {
 try { await init(); await connect(data.pair, data.name, data.memory); }
 catch (error) { self.postMessage(`Connector stopped: ${String(error)}`); self.close(); }
};
