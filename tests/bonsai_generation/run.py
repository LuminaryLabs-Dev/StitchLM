"""Full pinned Bonsai generation on four native or mixed native/WASM connectors.
Run each case sequentially. Native process address caps are one billion bytes.
"""
import argparse,json,os,pathlib,re,subprocess,time,urllib.request,urllib.error,threading,concurrent.futures
from playwright.sync_api import sync_playwright
p=argparse.ArgumentParser();p.add_argument('--tokenizer',required=True);p.add_argument('--axis',choices=['rows','columns'],default='rows');p.add_argument('--steps',type=int,default=2);p.add_argument('--cancel-check',action='store_true');p.add_argument('--gguf',required=True);p.add_argument('--browser',required=True);p.add_argument('--output',required=True);p.add_argument('--native-count',type=int,choices=[2,4],default=2);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output);out.mkdir(parents=True,exist_ok=True);port=18790;base=f'http://127.0.0.1:{port}';children=[];files=[];metrics={};sampling=True

def launch(name,args,env=None):
 f=open(out/(name+'.private.log'),'w');files.append(f);c=subprocess.Popen(args,stdout=f,stderr=subprocess.STDOUT,env=env,cwd=root);children.append(c);return c

def api(path,token='',data=None):
 r=urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method='GET'if data is None else'POST')
 try:
  with urllib.request.urlopen(r,timeout=3600)as response:return json.load(response)
 except urllib.error.HTTPError as e:
  print('API failed',path,e.code,e.read().decode(),flush=True);raise

def sample():
 while sampling:
  for c in list(children):
   try:
    status=pathlib.Path(f'/proc/{c.pid}/status').read_text();stat=pathlib.Path(f'/proc/{c.pid}/stat').read_text().split();m=metrics.setdefault(str(c.pid),{'max_rss_bytes':0,'max_virtual_bytes':0,'cpu_seconds':0,'first':time.monotonic(),'last':0});m['max_rss_bytes']=max(m['max_rss_bytes'],int(re.search(r'VmRSS:\s+(\d+)',status).group(1))*1024);m['max_virtual_bytes']=max(m['max_virtual_bytes'],int(re.search(r'VmSize:\s+(\d+)',status).group(1))*1024);m['cpu_seconds']=(int(stat[13])+int(stat[14]))/os.sysconf('SC_CLK_TCK');m['last']=time.monotonic()
   except (OSError,ValueError,AttributeError,IndexError):pass
  time.sleep(.05)
thread=threading.Thread(target=sample,daemon=True);thread.start();report={'synthetic_weights':False,'full_bonsai_generation':True,'native_connectors':a.native_count,'browser_connectors':4-a.native_count,'native_address_cap_bytes':1000000000,'browser_process_cap_enforced':False,'tests':{}}
try:
 host=launch('host',[str(root/'target/release/stitchlm-host'),'--model-file',a.gguf,'--tokenizer',a.tokenizer,'--context','64','--listen',f'127.0.0.1:{port}','--web-dir',str(root/'dist')])
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
  def monitor():
   while sampling:
    try:
     status=api('/api/shards/status',admin);commands=status.get('commands',[])
     (out/'live-status.json').write_text(json.dumps({'commands':len(commands),'completed':sum(c['status']=='completed'for c in commands),'active_operation':status.get('active_request')is not None,'timestamp':time.time()}))
    except Exception:pass
    time.sleep(15)
  threading.Thread(target=monitor,daemon=True).start()
  request={'prompt':'Hello','max_tokens':a.steps,'raw_prompt':True,'local_only':False,'connector_count':4,'split_axis':a.axis,'working_memory_bytes':32000000}
  if a.cancel_check:
   forced=[False];conflicts=[0]
   def cancel_during_browser_fetch(route):
    if not forced[0]:
     api('/api/cancel',admin,{});forced[0]=True
    route.continue_()
   if pages:
    context.route('**/shard-weights/**',cancel_during_browser_fetch)
    context.on('response',lambda response: conflicts.__setitem__(0,conflicts[0]+1)if '/shard-weights/'in response.url and response.status==409 else None)
   def cancellable_generation():
    req=urllib.request.Request(base+'/api/generate',data=json.dumps(request).encode(),headers={'Authorization':'Bearer '+admin,'Content-Type':'application/json'},method='POST')
    with urllib.request.urlopen(req,timeout=180)as r:return [json.loads(line)for line in r]
   with concurrent.futures.ThreadPoolExecutor(max_workers=1)as pool:
    future=pool.submit(cancellable_generation)
    for _ in range(600):
     if pages:
      pages[0].wait_for_timeout(20)
      if forced[0]:break
     else:
      current=api('/api/shards/status',admin)
      if current['active_request']and any(c['status']in['running','dispatched']for c in current['commands']):
       api('/api/cancel',admin,{});break
      time.sleep(.02)
    else:raise AssertionError('no remote weight request became active before cancellation')
    cancelled=future.result(timeout=180)
   if pages:
    for _ in range(100):
     pages[0].wait_for_timeout(50)
     if conflicts[0]>0:break
    assert conflicts[0]>0,'stale weight response was not observed'
    context.unroute('**/shard-weights/**',cancel_during_browser_fetch)
    for page in pages:
     text=page.locator('#status').inner_text().lower();assert 'error'not in text and 'failed'not in text,text
   else:time.sleep(.5)
   assert cancelled[-1]['kind']=='error'and'cancelled'in cancelled[-1]['text'],cancelled[-1]
   assert api('/api/status',admin)['active_request']is None
   assert all(c.poll()is None for c in children),'cancellation killed a process'
   report['tests']['distributed_cancel_and_fresh_replay']={'cancelled':True,'remote_command_was_active':True,'forced_browser_weight_fetch_cancel':forced[0],'stale_weight_conflicts_observed':conflicts[0]}
  if pages:
   console=context.new_page();console.goto(base+'/host-web/');console.locator('#access').fill(admin);console.locator('#refresh').click();console.wait_for_function("document.getElementById('status').textContent==='Host connected'")
   console.get_by_text('Generation options',exact=True).click();console.locator('#prompt-mode').select_option('raw');console.locator('#generation-axis').select_option(a.axis);console.locator('#prompt').fill('Hello');console.locator('#tokens').fill(str(a.steps));console.locator('#generate').click()
   console.wait_for_function("document.getElementById('status').textContent.startsWith('done |')||document.getElementById('status').textContent.startsWith('error |')",timeout=1800000)
   text=console.locator('#output').inner_text();status_text=console.locator('#status').inner_text();assert status_text.startswith('done |'),status_text+' '+text
   assert 'remote matrix shards' in status_text,status_text
   assert text==(', I' if a.steps==2 else ','),repr(text)
   report['tests']['full_generation']={'text':text,'status':status_text,'browser_console':True,'matches_pinned_reference':True}
   console.locator('#access').fill('');console.screenshot(path=str(out/'full-model-console.png'),full_page=True)
  else:
   req=urllib.request.Request(base+'/api/generate',data=json.dumps(request).encode(),headers={'Authorization':'Bearer '+admin,'Content-Type':'application/json'},method='POST')
   events=[]
   with urllib.request.urlopen(req,timeout=3600)as response:
    for line in response:
     event=json.loads(line);events.append(event);print(json.dumps(event),flush=True)
   assert events[-1]['kind']=='done',events[-1]
   tokens=[e['token']for e in events if e['kind']=='token'];assert tokens==[11,353][:a.steps],tokens
   assert events[-1]['remote_shards']>100 and events[-1]['remote_blocks']==0
   report['tests']['full_generation']={'events':events,'tokens':tokens,'matches_pinned_reference':True,'split_axis':a.axis}
  assert all(c.poll()is None for c in children),'host or native connector exited'
  status=api('/api/shards/status',admin);report['tests']['last_operation']=status
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
print(json.dumps({'passed':report.get('passed',False),'full_generation':report['tests'].get('full_generation'),'process_metrics':report['process_metrics']}))
