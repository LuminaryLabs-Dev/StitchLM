"""Actual Bonsai projection through data-driven commands on2native+2WASMworkers.
No full-model Bonsai inference claim. At most4connectors exist concurrently.
"""
import argparse,json,os,pathlib,re,subprocess,time,urllib.request,urllib.error,threading
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--model-dir',required=True);p.add_argument('--gguf',required=True);p.add_argument('--browser',required=True);p.add_argument('--output',required=True);p.add_argument('--native-count',type=int,choices=[2,4],default=2);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output);out.mkdir(parents=True,exist_ok=True);port=18790;base=f'http://127.0.0.1:{port}';children=[];files=[];metrics={};sampling=True

def launch(name,args,env=None):
 f=open(out/(name+'.private.log'),'w');files.append(f);c=subprocess.Popen(args,stdout=f,stderr=subprocess.STDOUT,env=env,cwd=root);children.append(c);return c

def api(path,token='',data=None):
 r=urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method='GET'if data is None else'POST')
 try:
  with urllib.request.urlopen(r,timeout=180)as response:return json.load(response)
 except urllib.error.HTTPError as e:
  print('API failed',path,e.code,e.read().decode(),flush=True);raise

def sample():
 while sampling:
  for c in list(children):
   try:
    status=pathlib.Path(f'/proc/{c.pid}/status').read_text();stat=pathlib.Path(f'/proc/{c.pid}/stat').read_text().split();m=metrics.setdefault(str(c.pid),{'max_rss_bytes':0,'max_virtual_bytes':0,'cpu_seconds':0,'first':time.monotonic(),'last':0});m['max_rss_bytes']=max(m['max_rss_bytes'],int(re.search(r'VmRSS:\s+(\d+)',status).group(1))*1024);m['max_virtual_bytes']=max(m['max_virtual_bytes'],int(re.search(r'VmSize:\s+(\d+)',status).group(1))*1024);m['cpu_seconds']=(int(stat[13])+int(stat[14]))/os.sysconf('SC_CLK_TCK');m['last']=time.monotonic()
   except (OSError,ValueError,AttributeError,IndexError):pass
  time.sleep(.05)
thread=threading.Thread(target=sample,daemon=True);thread.start();report={'synthetic_weights':False,'full_bonsai_generation':False,'native_connectors':a.native_count,'browser_connectors':4-a.native_count,'native_address_cap_bytes':1000000000,'browser_process_cap_enforced':False,'tests':{}}
try:
 host=launch('host',[str(root/'target/release/stitchlm-host'),'--model-dir',a.model_dir,'--gguf-resource',a.gguf,'--listen',f'127.0.0.1:{port}','--web-dir',str(root/'dist')])
 for _ in range(360):
  text=(out/'host.private.log').read_text();m=re.search(r'Host access code: (\w+)\nPairing code: (\w+)',text)
  if m:admin,pair=m.groups();break
  assert host.poll()is None,text;time.sleep(.5)
 else:raise RuntimeError('host startup timed out')
 env=os.environ.copy();env['STITCH_PAIRING_CODE']=pair;env['RAYON_NUM_THREADS']='1';env['TOKIO_WORKER_THREADS']='1'
 for n in range(a.native_count):launch(f'native{n}',['prlimit','--as=1000000000','--cpu=120','--','nice','-n','19',str(root/'target/release/stitchlm-connector'),'--host',base,'--name',f'native-shard-{n}','--memory-mb','953','--cpu-percent','10'],env)
 with sync_playwright()as p:
  browser=p.chromium.launch(executable_path=a.browser,headless=True,args=['--no-sandbox','--disable-dev-shm-usage']);context=browser.new_context(viewport={'width':1100,'height':850});pages=[]
  for n in range(4-a.native_count):
   page=context.new_page();page.goto(base+'/connector-web/');page.locator('#pair').fill(pair);page.locator('#name').fill(f'web-shard-{n}');page.locator('#memory').fill('953');page.locator('#connect').click();page.wait_for_function("document.getElementById('status').textContent.startsWith('Ready. Computing')",timeout=120000);pages.append(page)
  for _ in range(200):
   status=api('/api/status',admin)
   if sum(c['ready']for c in status['connectors'])==4:break
   time.sleep(.1)
  assert len(status['connectors'])==4 and all(c['ready']for c in status['connectors'])
  inputs=[((i%31)-15)/32 for i in range(5120)]
  for axis,preferred in [('rows',1024),('columns',1280)]:
   result=api('/api/shards/project',admin,{'target':{'kind':'registered','resource_id':'bonsai-ffn-gate'},'input':inputs,'connector_count':4,'memory_limit_bytes':1000000000,'max_working_bytes':10000000,'preferred_chunk':preferred,'split_axis':axis})
   assert len(result['output'])==17408 and not result['full_model_inference'];assert all(c['status']=='completed'for c in result['commands']);assert len(set(c['command']['device_id']for c in result['commands']))==4
   assert len(result['commands'])>4,'multi-round queue wasnotexercised'
   assert all(d['budget']['estimated_peak']<=10000000 for d in result['definitions'])
   assert all(d['parent_id']for d in result['definitions'])
   assert all(c['started_ms']is not None and c['completed_ms']is not None for c in result['commands'])
   assert all(d['measurements']for d in result['device_measurements'])
   (out/f'{axis}-full-report.json').write_text(json.dumps(result,indent=2))
   report['tests'][axis]={'commands':len(result['commands']),'devices':len(set(c['command']['device_id']for c in result['commands'])),'maximum_reference_error':result['maximum_reference_error'],'elapsed_ms':result['elapsed_ms'],'max_estimated_working_bytes':max(d['budget']['estimated_peak']for d in result['definitions']),'checkpoint_checksum':result['checkpoint']['checksum']}
   restored=api('/api/shards/restore',admin,result['checkpoint']);assert restored['restored']and restored['completed']==len(result['commands'])and not restored['replay_required'];report['tests'][axis]['checkpoint_restored']=True
   tampered=json.loads(json.dumps(result['checkpoint']));tampered['token_position']+=1
   try:api('/api/shards/restore',admin,tampered);raise AssertionError('tamperedcheckpointaccepted')
   except urllib.error.HTTPError as e:assert e.code==400
  if pages:
   console=context.new_page();console.goto(base+'/host-web/');console.locator('#access').fill(admin);console.locator('#refresh').click();console.wait_for_function("document.getElementById('status').textContent==='Host connected'")
   console.locator('#run-shards').click();console.wait_for_function("document.getElementById('status').textContent.startsWith('Shard execution passed')",timeout=60000)
   ui_report=json.loads(console.locator('#shard-report').inner_text());assert ui_report['command_count']==4;assert ui_report['maximum_reference_error']<.002
   console.screenshot(path=str(out/'shard-console.png'),full_page=True);console.locator('#restore-shards').click();console.wait_for_function("document.getElementById('status').textContent.startsWith('Checkpoint inspected')")
   report['tests']['host_console']={'synthetic_matrix':True,'commands':4,'checkpoint_restored':True}
  for n,page in enumerate(pages):page.screenshot(path=str(out/f'web-shard-{n}.png'),full_page=True);page.locator('#disconnect').click()
  browser.close();report['passed']=True
finally:
 for c in reversed(children):
  if c.poll()is None:
   c.terminate()
   try:c.wait(timeout=5)
   except subprocess.TimeoutExpired:c.kill();c.wait()
 sampling=False;thread.join(timeout=2);report['process_metrics']=[]
 for i,c in enumerate(children):
  m=metrics.get(str(c.pid),{});duration=m.pop('last',0)-m.pop('first',0);m['role']='host'if i==0 else'native-connector';m['sampled_seconds']=max(0,duration);m['average_single_core_cpu_percent']=m.get('cpu_seconds',0)/duration*100 if duration>0 else None;report['process_metrics'].append(m)
 for f in files:f.close()
 (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
