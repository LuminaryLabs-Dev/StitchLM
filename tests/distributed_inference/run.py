"""Real native host plus four independent connector processes, recovery and auth checks."""
import argparse,json,os,pathlib,re,subprocess,time,urllib.request,urllib.error,concurrent.futures,threading
p=argparse.ArgumentParser();p.add_argument('--model-dir',required=True);p.add_argument('--output',required=True);p.add_argument('--port',type=int,default=18787);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output);out.mkdir(parents=True,exist_ok=True);base=f'http://127.0.0.1:{a.port}';children=[];handles=[]
def api(path,token='',data=None):
 req=urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'},method='GET'if data is None else'POST')
 with urllib.request.urlopen(req,timeout=90)as response:return response.read().decode()
def launch(name,args,env=None):
 f=open(out/(name+'.private.log'),'w');handles.append(f);child=subprocess.Popen(args,cwd=root,stdout=f,stderr=subprocess.STDOUT,env=env);children.append(child);return child
metrics={};sampling=True
def sample():
 while sampling:
  for child in list(children):
   try:
    stat=pathlib.Path(f'/proc/{child.pid}/stat').read_text().split();status=pathlib.Path(f'/proc/{child.pid}/status').read_text();rss=int(re.search(r'VmRSS:\s+(\d+)',status).group(1))*1024;vms=int(re.search(r'VmSize:\s+(\d+)',status).group(1))*1024
    m=metrics.setdefault(str(child.pid),{'max_rss_bytes':0,'max_virtual_bytes':0,'cpu_seconds':0,'first_sample':time.monotonic(),'last_sample':0});m['max_rss_bytes']=max(m['max_rss_bytes'],rss);m['max_virtual_bytes']=max(m['max_virtual_bytes'],vms);m['cpu_seconds']=(int(stat[13])+int(stat[14]))/os.sysconf('SC_CLK_TCK');m['last_sample']=time.monotonic()
   except (OSError,AttributeError,IndexError,ValueError):pass
  time.sleep(.025)
thread=threading.Thread(target=sample,daemon=True);thread.start()
report={'synthetic':False,'connector_limit':4,'per_connector_virtual_address_cap_bytes':1000000000,'cpu_duty_target_percent':10,'cpu_burst_cap':False,'tests':{}}
try:
 host=launch('host',[str(root/'target/release/stitchlm-host'),'--model-dir',a.model_dir,'--listen',f'127.0.0.1:{a.port}','--web-dir',str(root/'dist')]);admin=pair=None
 for _ in range(180):
  log=(out/'host.private.log').read_text();m=re.search(r'Host access code: (\w+)\nPairing code: (\w+)',log)
  if m:admin,pair=m.groups();break
  if host.poll()is not None:raise RuntimeError('host exited: '+log)
  time.sleep(.5)
 assert admin and pair,'host didnotbecomeready'
 for _ in range(30):
  try:api('/api/status',admin);break
  except OSError:time.sleep(.2)
 try:api('/api/status');raise AssertionError('unauthorizedstatusaccepted')
 except urllib.error.HTTPError as e:assert e.code==401
 report['tests']['unauthorized_rejected']=True
 env=os.environ.copy();env['STITCH_PAIRING_CODE']=pair;env['RAYON_NUM_THREADS']='1';env['TOKIO_WORKER_THREADS']='1'
 for n in range(4):launch(f'connector{n}',['prlimit','--as=1000000000','--cpu=60','--','nice','-n','19',str(root/'target/release/stitchlm-connector'),'--cpu-percent','10','--host',base,'--name',f'native-{n}','--memory-mb','953'],env)
 for _ in range(180):
  status=json.loads(api('/api/status',admin))
  if sum(c['ready']for c in status['connectors'])==4:break
  time.sleep(.25)
 assert sum(c['ready']for c in status['connectors'])==4,'four workers not ready'
 request={'prompt':'Reply with the word hello.','max_tokens':12,'local_only':True}
 local=[json.loads(x)for x in api('/api/generate',admin,request).splitlines()];assert local[-1]['kind']=='done',local
 request['local_only']=False;remote=[json.loads(x)for x in api('/api/generate',admin,request).splitlines()];assert remote[-1]['kind']=='done',remote
 assert [e['token']for e in local if e['kind']=='token']==[e['token']for e in remote if e['kind']=='token']
 assert remote[-1]['remote_blocks']>0
 report['tests']['native_network_parity']={'local':local,'distributed':remote,'exact_tokens':True,'independent_worker_processes':4}
 # Removeoneworkerbetweenrequests. Host must use intactremainingworker+localblocks.
 children[1].terminate();children[1].wait(timeout=10)
 status=json.loads(api('/api/status',admin));dead=next(c['id']for c in status['connectors']if c['name']=='native-0')
 api(f'/api/connectors/{dead}/remove',admin,{})
 recovery=[json.loads(x)for x in api('/api/generate',admin,request).splitlines()];assert recovery[-1]['kind']=='done';assert recovery[-1]['text']==local[-1]['text']
 report['tests']['remove_and_reprefill']={'passed':True,'events':recovery}
 report['status']='passed'
 for _ in range(3):
  more=[json.loads(x)for x in api('/api/generate',admin,request).splitlines()];assert more[-1]['kind']=='done';assert more[-1]['text']==local[-1]['text']
 report['tests']['repeated_requests']=3
 # Cancel an active request; require an explicit error event, then fresh remote prefill.
 with concurrent.futures.ThreadPoolExecutor(max_workers=1)as pool:
  future=pool.submit(api,'/api/generate',admin,{'prompt':'Explain why a clear sky is blue. '*20,'max_tokens':32,'local_only':False})
  for _ in range(100):
   if json.loads(api('/api/status',admin))['active_request']:break
   time.sleep(.01)
  api('/api/cancel',admin,{})
  cancelled=[json.loads(x)for x in future.result().splitlines()]
 assert cancelled[-1]['kind']=='error'and'cancelled'in cancelled[-1]['text']
 report['tests']['cancelled_request']=cancelled[-1]
 for _ in range(300):
  health=json.loads(api('/api/status',admin))
  if len(health['connectors'])==3 and all(c['ready']for c in health['connectors']):break
  time.sleep(.1)
 assert all(c.poll()is None for c in children[2:]),'cancellation killed a retained connector'
 assert all(c['ready']for c in health['connectors']),'connectors did not reset after cancellation'
 recovered=[json.loads(x)for x in api('/api/generate',admin,request).splitlines()];assert recovered[-1]['kind']=='done';assert recovered[-1]['text']==local[-1]['text'];assert recovered[-1]['remote_blocks']>0
 report['tests']['post_cancel_remote_blocks']=recovered[-1]['remote_blocks']
 report['tests']['post_cancel_reprefill']=True
finally:
 for c in reversed(children):
  if c.poll()is None:
   c.terminate()
   try:c.wait(timeout=5)
   except subprocess.TimeoutExpired:c.kill();c.wait()
 sampling=False;thread.join(timeout=2)
 report['process_metrics']=[]
 for index,c in enumerate(children):
  m=metrics.get(str(c.pid),{});duration=m.pop('last_sample',0)-m.pop('first_sample',0)
  m['sampled_seconds']=max(0,duration);m['average_single_core_cpu_percent']=m.get('cpu_seconds',0)/duration*100 if duration>0 else None;m['role']='host'if index==0 else'connector';report['process_metrics'].append(m)
 for f in handles:f.close()
 (out/'network-report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
