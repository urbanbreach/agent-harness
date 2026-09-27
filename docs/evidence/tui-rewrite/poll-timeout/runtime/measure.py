import hashlib, importlib.util, json, pathlib, shutil, statistics, subprocess, time
root=pathlib.Path('/home/urbanbreach/Projects/agent-harness')
reference_root=pathlib.Path('/home/urbanbreach/.codex/worktrees/tui-reference/agent-harness')
out=pathlib.Path('/tmp/tui-poll-timeout-runtime');out.mkdir(exist_ok=True)
reference=reference_root/'target/release/examples/poll_timeout_resource_probe'
binaries={'reference':reference,'before':pathlib.Path('/tmp/tui-runtime-90739ad2-resource-probe.bin'),'candidate':pathlib.Path('/tmp/tui-poll-timeout-resource-probe.bin')}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
fixture=root/'crates/harness-tui/examples/resource_probe.rs';reference_fixture=reference_root/'crates/harness-tui/examples/poll_timeout_resource_probe.rs';assert sha(fixture)==sha(reference_fixture)
metadata={'candidate_base':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'candidate_patch':str(out/'source.patch'),'reference_commit':'1bb0f98988670a5f4b48cdf749b455a79cfdaa82','binaries':{label:{'path':str(path),'sha256':sha(path),'bytes':path.stat().st_size} for label,path in binaries.items()},'runner_sha256':sha(root/'scripts/measure-tui-runtime.py'),'fixture_sha256':sha(fixture),'reference_fixture_sha256':sha(reference_fixture),'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'seconds':4,'warmup_seconds':1.5,'repetitions':3,'cadence_override':None,'boundary':'process CPU and RSS through PTY; output frame delivery, no display emulator; disposable fixture terminated by SIGTERM, cleanup verified separately','input_caveat':'unchanged frozen driver targets 1ms typing but actual input counts depend on event-loop scheduling; report raw counts and do not treat typing CPU alone as fixed-count efficiency'}
metadata['vendor_files']={str(p.relative_to(root)):sha(p) for p in (root/'vendor/filedescriptor').rglob('*') if p.is_file() and 'target' not in p.parts and p.name!='Cargo.lock'}
(out/'metadata.json').write_text(json.dumps(metadata,indent=2)+'\n');(out/'source.patch').write_text(subprocess.check_output(['git','diff','--binary'],cwd=root,text=True))
spec=importlib.util.spec_from_file_location('probe',root/'scripts/measure-tui-runtime.py');probe=importlib.util.module_from_spec(spec);spec.loader.exec_module(probe)
samples=[]
for scenario in ('idle','startup','typing','burst','slow-burst'):
 for rep in range(3):
  order=['reference','before','candidate'] if rep!=1 else ['candidate','before','reference']
  for label in order:
   sample={'build':label,'repetition':rep+1,'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),**probe.measure(binaries[label],scenario,4,None)}
   samples.append(sample);print(json.dumps(sample),flush=True);(out/'samples.json').write_text(json.dumps(samples,indent=2)+'\n')
summary={}
for scenario in ('idle','startup','typing','burst','slow-burst'):
 summary[scenario]={}
 for label in binaries:
  rows=[r for r in samples if r['scenario']==scenario and r['build']==label]
  summary[scenario][label]={key:statistics.median([r[key] for r in rows if r[key] is not None]) if any(r[key] is not None for r in rows) else None for key in ('cpu_percent_one_core','rss_before_kib','rss_after_kib','frames','frames_per_second','bytes','input_events','frame_interval_p95_ms')}
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({'summary':summary}),flush=True)
assert all(r['cpu_percent_one_core']==0 and r['frames']==0 and r['bytes']==0 for r in samples if r['build']=='candidate' and r['scenario']=='idle'), 'settled idle is not quiescent'
