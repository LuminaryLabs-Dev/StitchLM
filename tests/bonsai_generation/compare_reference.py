"""Compare independent Prism and Rust full-model token/logit fixtures."""
import argparse,json,pathlib
import numpy as np
p=argparse.ArgumentParser();p.add_argument('reference');p.add_argument('candidate');p.add_argument('--output',required=True);a=p.parse_args();ref=pathlib.Path(a.reference);candidate=pathlib.Path(a.candidate);run=json.loads((candidate/'report.json').read_text())
inputs=[int(x)for x in(ref/'input.txt').read_text().split()];generated=[int(x)for x in(ref/'generated.txt').read_text().split()]
assert inputs==run['input'],'tokenization differs';assert generated==run['generated'],'greedy continuation differs'
results=[]
for position in range(len(run['steps'])):
 x=np.fromfile(ref/f'logits-{position}.f32',dtype='<f4').astype(np.float64);y=np.fromfile(candidate/f'logits-{position}.f32',dtype='<f4').astype(np.float64)
 assert len(x)==len(y)==248320 and np.isfinite(x).all()and np.isfinite(y).all()
 cosine=float(x@y/(np.linalg.norm(x)*np.linalg.norm(y)));normalized=float(np.linalg.norm(x-y)/np.linalg.norm(x));same=int(x.argmax())==int(y.argmax())
 assert same and cosine>=.999 and normalized<=.02,(position,same,cosine,normalized)
 results.append({'position':position,'same_top1':same,'cosine':cosine,'normalized_rmse':normalized,'maximum_absolute_error':float(abs(x-y).max())})
result={'passed':True,'independent_reference':True,'reference_commit':'b06f74a9ea9fa5eaf751f7d575bf766486b4ae62','input_tokens':inputs,'generated_tokens':generated,'text':run['text'],'positions':results,'gate':{'cosine_minimum':.999,'normalized_rmse_maximum':.02,'greedy_match_required':True},'limitation':'Named short fixtures only; F32 Rust and Q8-activation reference logits are not bitwise identical.'}
pathlib.Path(a.output).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'passed':True,'positions':len(results),'text':run['text']}))
