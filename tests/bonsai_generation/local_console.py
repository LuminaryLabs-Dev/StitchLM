"""Actual browser console, normal Bonsai chat template, cancellation and fresh replay.
Independent reference-generated token IDs must be supplied after that run finishes.
No connectors are created here; this is explicitly the host-local acceptance path.
"""
import argparse,json,pathlib,re,subprocess,time,urllib.request
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--gguf',required=True);p.add_argument('--tokenizer',required=True);p.add_argument('--browser',required=True);p.add_argument('--reference-tokens',required=True);p.add_argument('--output',required=True);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output);out.mkdir(parents=True,exist_ok=True);base='http://127.0.0.1:18791';log=open(out/'host.private.log','w');host=subprocess.Popen([str(root/'target/release/stitchlm-host'),'--model-file',a.gguf,'--tokenizer',a.tokenizer,'--listen','127.0.0.1:18791','--context','64','--web-dir',str(root/'dist')],cwd=root,stdout=log,stderr=subprocess.STDOUT);report={'full_model':True,'host_local_only':True,'physical_lan':False}
try:
 for _ in range(360):
  text=(out/'host.private.log').read_text();m=re.search(r'Host access code: (\w+)',text)
  if m:admin=m[1];break
  assert host.poll()is None,'host exited';time.sleep(.5)
 else:raise RuntimeError('host readiness timeout')
 def health():
  req=urllib.request.Request(base+'/api/status',headers={'Authorization':'Bearer '+admin})
  with urllib.request.urlopen(req,timeout=10)as r:return json.load(r)
 with sync_playwright()as pw:
  browser=pw.chromium.launch(executable_path=a.browser,headless=True,args=['--no-sandbox','--disable-dev-shm-usage']);page=browser.new_page(viewport={'width':1100,'height':950});errors=[];page.on('pageerror',lambda e:errors.append(str(e)))
  page.goto(base+'/host-web/');page.locator('#access').fill(admin);page.locator('#refresh').click();page.wait_for_function("document.getElementById('status').textContent==='Host connected'")
  assert page.locator('#prompt-mode').input_value()=='chat';page.locator('#prompt').fill('Hi');page.locator('#tokens').fill('2');page.locator('#generate').click()
  for _ in range(100):
   if health()['active_request']:break
   time.sleep(.02)
  page.locator('#cancel').click();page.wait_for_function("document.getElementById('output').textContent.includes('generation cancelled')",timeout=120000);assert health()['active_request']is None;report['cancellation']=True
  with page.expect_response('**/api/generate')as response_info:page.locator('#generate').click()
  response=response_info.value
  page.wait_for_function("document.getElementById('status').textContent.startsWith('done |')||document.getElementById('status').textContent.startsWith('error |')",timeout=1800000)
  events=[json.loads(line)for line in response.body().decode().splitlines()];assert events[-1]['kind']=='done',events[-1]
  tokens=[e['token']for e in events if e['kind']=='token'];expected=[int(x)for x in pathlib.Path(a.reference_tokens).read_text().split()];assert tokens==expected,(tokens,expected)
  assert events[-1]['remote_shards']==0 and events[-1]['remote_blocks']==0
  assert not errors,errors;report.update({'passed':True,'normal_chat_template':True,'fresh_request_after_cancel':True,'tokens':tokens,'text':events[-1]['text'],'events':events,'browser_errors':errors})
  page.locator('#access').fill('');page.screenshot(path=str(out/'normal-chat.png'),full_page=True);browser.close()
finally:
 host.terminate()
 try:host.wait(timeout=5)
 except subprocess.TimeoutExpired:host.kill();host.wait()
 log.close();(out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
