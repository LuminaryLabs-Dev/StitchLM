"""Real browser/WASM connector acceptance, using local committed-source candidate."""
import argparse,json,pathlib,re,subprocess,time,urllib.request
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--model-dir',required=True);p.add_argument('--output',required=True);p.add_argument('--browser',required=True);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output);out.mkdir(parents=True,exist_ok=True);port=18789;base=f'http://127.0.0.1:{port}';log=open(out/'host.private.log','w');host=subprocess.Popen([str(root/'target/release/stitchlm-host'),'--model-dir',a.model_dir,'--listen',f'127.0.0.1:{port}','--web-dir',str(root/'dist')],stdout=log,stderr=subprocess.STDOUT,cwd=root)
report={'synthetic':False,'browser':a.browser,'errors':[]}
try:
 for _ in range(180):
  text=(out/'host.private.log').read_text();m=re.search(r'Host access code: (\w+)\nPairing code: (\w+)',text)
  if m:admin,pair=m.groups();break
  assert host.poll()is None,'host exited';time.sleep(.25)
 else:raise RuntimeError('host startup timeout')
 with sync_playwright()as p:
  browser=p.chromium.launch(executable_path=a.browser,headless=True,args=['--no-sandbox','--disable-dev-shm-usage']);context=browser.new_context(viewport={'width':1100,'height':850});console=context.new_page();connector=context.new_page()
  for page in [console,connector]:page.on('pageerror',lambda e:report['errors'].append(str(e)))
  console.goto(base+'/host-web/');console.locator('#access').fill(admin);console.locator('#refresh').click();console.wait_for_function("document.getElementById('status').textContent==='Host connected'",timeout=30000)
  connector.goto(base+'/connector-web/');connector.locator('#pair').fill(pair);connector.locator('#name').fill('browser-proof');connector.locator('#memory').fill('953');connector.locator('#connect').click();connector.wait_for_function("document.getElementById('status').textContent.startsWith('Ready. Computing')",timeout=120000)
  console.locator('#refresh').click();console.wait_for_function("document.getElementById('devices').textContent.includes('browser-proof')")
  console.locator('#prompt').fill('Reply with the word hello.');console.locator('#tokens').fill('12');console.locator('#generate').click();console.wait_for_function("document.getElementById('status').textContent.startsWith('done |')||document.getElementById('status').textContent.startsWith('error |')",timeout=120000)
  report['generation_status']=console.locator('#status').inner_text();report['output']=console.locator('#output').inner_text();report['connector_status']=connector.locator('#status').inner_text();assert report['generation_status'].startswith('done |'),report;assert report['output']=='hello',report;assert not report['generation_status'].endswith('| 0 remote block executions'),report
  console.locator('#prompt').fill('Explain why the sky is blue. '*20);console.locator('#tokens').fill('32');console.locator('#generate').click()
  def health():
   req=urllib.request.Request(base+'/api/status',headers={'Authorization':'Bearer '+admin})
   with urllib.request.urlopen(req,timeout=10)as response:return json.load(response)
  for _ in range(100):
   if health()['active_request']:break
   time.sleep(.02)
  console.locator('#cancel').click();console.wait_for_function("document.getElementById('output').textContent.includes('generation cancelled')",timeout=30000)
  for _ in range(300):
   if all(c['ready']for c in health()['connectors']):break
   time.sleep(.1)
  assert all(c['ready']for c in health()['connectors']),'browser connector did not reset'
  console.locator('#prompt').fill('Reply with the word hello.');console.locator('#tokens').fill('12');console.locator('#generate').click();console.wait_for_function("document.getElementById('status').textContent.startsWith('done |')",timeout=120000)
  assert console.locator('#output').inner_text()=='hello';assert not console.locator('#status').inner_text().endswith('| 0 remote block executions');report['browser_cancel_and_remote_resume']=True
  console.screenshot(path=str(out/'host-console.png'),full_page=True);connector.screenshot(path=str(out/'web-connector.png'),full_page=True)
  connector.locator('#disconnect').click();assert connector.locator('#status').inner_text().startswith('Worker stopped');report['stop_worker']=True;report['passed']=not report['errors'];browser.close()
finally:
 host.terminate()
 try:host.wait(timeout=5)
 except subprocess.TimeoutExpired:host.kill();host.wait()
 log.close();(out/'browser-report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
