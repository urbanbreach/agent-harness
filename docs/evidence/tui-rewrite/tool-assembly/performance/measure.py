from pathlib import Path
import copy, datetime, hashlib, json, os, re, shutil, statistics, subprocess, sys
root=Path('/home/urbanbreach/Projects/agent-harness')
side=sys.argv[1]
out=Path('/tmp/tui-tool-assembly-performance');folder=out/side;folder.mkdir(parents=True,exist_ok=True)
metadata=json.loads(Path(f'/tmp/tui-tool-assembly-{side}-binaries.json').read_text())
source_binary=next(iter(metadata['rust-binaries'].values()))['binary-path']
if side=='reference': metadata=json.loads(Path('/tmp/tui-tool-assembly-before-binaries.json').read_text())
binary=next(iter(metadata['rust-binaries'].values()))
retained=Path(f'/tmp/tui-tool-assembly-{side}.bin');shutil.copy2(source_binary,retained);binary['binary-path']=str(retained)
(folder/'nextest-binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
(folder/'binary.json').write_text(json.dumps({'path':str(retained),'sha256':hashlib.sha256(retained.read_bytes()).hexdigest(),'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=(Path('/home/urbanbreach/.codex/worktrees/tui-reference/agent-harness') if side=='reference' else root),text=True).strip(),'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat()},indent=2)+'\n')
cmd=['cargo','nextest','run','--binaries-metadata',str(folder/'nextest-binaries.json'),'--cargo-metadata','/tmp/tui-settled-cargo.json','--profile','perf','-j1','--success-output','immediate']
order=[]
for sample in ['1','2','3','alloc']:
 env={**os.environ,'HARNESS_REWRITE_SCENARIO':'tools','HARNESS_REWRITE_HISTORY':'200','HARNESS_REWRITE_FRAMES':'200','HARNESS_REWRITE_PERF_OUT':str(folder/f'tools-200-{sample}.json')}
 command=cmd if sample!='alloc' else ['memusage','-n',retained.name,'--no-timer',*cmd]
 entry={'sample':sample,'command':command,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat()}
 with (folder/f'tools-200-{sample}.log').open('w') as log:r=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 entry['exit_code']=r.returncode;order.append(entry);(folder/'run-order.json').write_text(json.dumps(order,indent=2)+'\n')
 print(side,sample,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
samples=[json.loads((folder/f'tools-200-{rep}.json').read_text()) for rep in range(1,4)]
summary={key:statistics.median(s[key] for s in samples) for key in ['construction_us','cold_us','p50_us','p95_us','p99_us','bytes']}
summary['rss_kib']=statistics.median(s['after']['rss_kib'] for s in samples)
summary['cpu_ms_per_frame']=statistics.median((s['after']['cpu_ticks']-s['before']['cpu_ticks'])*1000/os.sysconf('SC_CLK_TCK')/200 for s in samples)
m=re.search(r'heap total:\s*(\d+), heap peak:\s*(\d+)',(folder/'tools-200-alloc.log').read_text());assert m
summary['allocated_bytes'],summary['peak_heap_bytes']=map(int,m.groups())
(folder/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
if side=='before':
 limits={k:summary[k]*1.10 for k in ['p95_us','p99_us','rss_kib','cpu_ms_per_frame','allocated_bytes','peak_heap_bytes']}
 (out/'acceptance-before-implementation.json').write_text(json.dumps({'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'baseline':summary,'candidate_limits':limits,'scope':'Tool projection and rendering through explicit global disclosure seam; no terminal emulator or input-latency claim.'},indent=2)+'\n')
print(json.dumps(summary,indent=2))
