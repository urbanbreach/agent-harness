import hashlib, importlib.util, json, pathlib, shutil, statistics, subprocess, time
root=pathlib.Path('/home/urbanbreach/Projects/agent-harness')
out=pathlib.Path('/tmp/tui-runtime-90739ad2'); out.mkdir(exist_ok=True)
reference=pathlib.Path('/home/urbanbreach/.codex/worktrees/tui-reference/agent-harness/target/release/examples/resource_probe')
candidate=pathlib.Path('/tmp/tui-runtime-90739ad2-resource-probe.bin')
shutil.copy2(root/'target/release/examples/resource_probe',candidate)
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('probe',root/'scripts/measure-tui-runtime.py');probe=importlib.util.module_from_spec(spec); spec.loader.exec_module(probe)
metadata={'candidate_commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'reference_commit':'1bb0f98988670a5f4b48cdf749b455a79cfdaa82','binaries':{label:{'path':str(path),'sha256':sha(path),'bytes':path.stat().st_size} for label,path in [('reference',reference),('candidate',candidate)]},'runner_sha256':sha(root/'scripts/measure-tui-runtime.py'),'fixture_sha256':sha(root/'crates/harness-tui/examples/resource_probe.rs'),'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'seconds':4,'warmup_seconds':1.5,'repetitions':3,'cadence_override':None}
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
samples=[]
for scenario in ('idle','startup','typing','burst','slow-burst'):
 for rep in range(3):
  for label,binary in ([('reference',reference),('candidate',candidate)] if rep != 1 else [('candidate',candidate),('reference',reference)]):
   sample={'build':label,'repetition':rep+1,'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),**probe.measure(binary,scenario,4,None)}
   samples.append(sample); print(json.dumps(sample),flush=True);(out/'samples.json').write_text(json.dumps(samples,indent=2)+'\n')
summary={}
for scenario in ('idle','startup','typing','burst','slow-burst'):
 summary[scenario]={}
 for label in ('reference','candidate'):
  rows=[r for r in samples if r['scenario']==scenario and r['build']==label]
  summary[scenario][label]={key:statistics.median([r[key] for r in rows if r[key] is not None]) if any(r[key] is not None for r in rows) else None for key in ('cpu_percent_one_core','rss_before_kib','rss_after_kib','frames','frames_per_second','bytes','input_events','frame_interval_p95_ms')}
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'summary':summary}),flush=True)
