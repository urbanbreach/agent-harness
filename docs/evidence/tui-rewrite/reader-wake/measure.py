from pathlib import Path
import importlib.util,subprocess,json,statistics,hashlib
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-reader-wake');base=root/'.omo/evidence/tui-rewrite/reader-wake'
base.mkdir(parents=True,exist_ok=True)
spec=importlib.util.spec_from_file_location('runtime',root/'scripts/measure-tui-runtime.py');runtime=importlib.util.module_from_spec(spec);spec.loader.exec_module(runtime)
resources=[]
for repetition in range(1,4):
 for label in (('before','candidate') if repetition!=2 else ('candidate','before')):
  binary=out/('before-resource_probe' if label=='before' else 'resource_probe')
  sample=runtime.measure(binary,'idle',8,None)
  resources.append(dict(build=label,repetition=repetition,**sample));(out/'idle-resources.json').write_text(json.dumps(resources,indent=2)+'\n')
  print(label,'idle',sample,flush=True)
  assert sample['cpu_percent_one_core']==0 and sample['frames']==0 and sample['bytes']==0
reports=[]
for repetition in range(1,4):
 for label in (('before','candidate') if repetition!=2 else ('candidate','before')):
  binary=out/('before-stop-probe' if label=='before' else 'rewrite_probe');path=base/f'{label}-{repetition}'
  with (out/f'browser-{label}-{repetition}.log').open('w') as log:
   subprocess.run(['node','scripts/qa/measure-rewrite-latency.mjs',str(binary),str(path),'120'],cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
  report=json.loads((path/'latency.json').read_text());reports.append(dict(build=label,repetition=repetition,path=str(path),summary=report['summary'],cleanup=report['cleanup'],browser_cleanup=report['browser_cleanup'],idle_output_bytes=report['idle_output_bytes']))
  (out/'browser-receipts.json').write_text(json.dumps(reports,indent=2)+'\n');print(label,repetition,report['summary'],flush=True)
summary={label:{name:{p:statistics.median(r['summary'][name][p] for r in reports if r['build']==label) for p in ('p50_ms','p95_ms','p99_ms')} for name in reports[0]['summary']} for label in ('before','candidate')}
(out/'browser-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
frozen=json.loads((root/'docs/evidence/tui-rewrite/poll-timeout/browser/acceptance.json').read_text());checks=[]
for old in frozen:
 name,p=old['workload'],old['percentile'];candidate=summary['candidate'][name][p];before=summary['before'][name][p]
 checks.append(dict(workload=name,percentile=p,candidate_ms=candidate,before_ms=before,frozen_reference_ms=old['frozen_reference_ms'],allowance_ms=16.7,before_pass=candidate<=before+16.7,frozen_pass=candidate<=old['frozen_reference_ms']+16.7))
(out/'browser-acceptance.json').write_text(json.dumps(checks,indent=2)+'\n');assert all(c['before_pass'] and c['frozen_pass'] for c in checks)
