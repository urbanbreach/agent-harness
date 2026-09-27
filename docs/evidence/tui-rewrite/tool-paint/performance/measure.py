from pathlib import Path
import copy,datetime,hashlib,json,os,re,shutil,statistics,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-tool-paint/performance');out.mkdir(exist_ok=True)
metadata=json.loads(Path('/tmp/tui-tool-paint-candidate-binaries.json').read_text())
new_binary=Path('/tmp/tui-tool-paint-candidate.bin')
shutil.copy2(next(iter(metadata['rust-binaries'].values()))['binary-path'],new_binary)
sources={'reference':('/tmp/tui-tool-assembly-reference.bin','1bb0f98988670a5f4b48cdf749b455a79cfdaa82'),'before':('/tmp/tui-tool-layout-candidate.bin','a7d6d8e17168ea339b9d660456b308f3839d3ff1'),'candidate':(str(new_binary),'a7d6d8e17168ea339b9d660456b308f3839d3ff1 plus source.patch')}
commands={};order=[]
for side,(path,source) in sources.items():
 folder=out/side;folder.mkdir(exist_ok=True);m=copy.deepcopy(metadata)
 next(iter(m['rust-binaries'].values()))['binary-path']=path
 (folder/'nextest-binaries.json').write_text(json.dumps(m,indent=2)+'\n')
 (folder/'binary.json').write_text(json.dumps({'path':path,'source':source,'sha256':hashlib.sha256(Path(path).read_bytes()).hexdigest()},indent=2)+'\n')
 commands[side]=['cargo','nextest','run','--binaries-metadata',str(folder/'nextest-binaries.json'),'--cargo-metadata','/tmp/tui-settled-cargo.json','--profile','perf','-j1','--success-output','immediate']
for sample in ['1','2','3','alloc']:
 for side in (['candidate','before','reference'] if sample=='2' else ['reference','before','candidate']):
  folder=out/side;env={**os.environ,'HARNESS_REWRITE_SCENARIO':'tools','HARNESS_REWRITE_HISTORY':'200','HARNESS_REWRITE_FRAMES':'200','HARNESS_REWRITE_PERF_OUT':str(folder/f'tools-200-{sample}.json')}
  command=commands[side]
  if sample=='alloc':command=['memusage','-n',Path(sources[side][0]).name,'--no-timer',*command]
  entry={'side':side,'sample':sample,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command}
  with (folder/f'tools-200-{sample}.log').open('w') as log:r=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
  entry['exit_code']=r.returncode;order.append(entry);(out/'run-order.json').write_text(json.dumps(order,indent=2)+'\n')
  print(side,sample,r.returncode,flush=True)
  if r.returncode:raise SystemExit(r.returncode)
summaries={}
for side in sources:
 folder=out/side;samples=[json.loads((folder/f'tools-200-{rep}.json').read_text()) for rep in range(1,4)]
 summary={key:statistics.median(s[key] for s in samples) for key in ['construction_us','cold_us','p50_us','p95_us','p99_us','bytes']}
 summary['rss_kib']=statistics.median(s['after']['rss_kib'] for s in samples)
 summary['cpu_ms_per_frame']=statistics.median((s['after']['cpu_ticks']-s['before']['cpu_ticks'])*1000/os.sysconf('SC_CLK_TCK')/200 for s in samples)
 m=re.search(r'heap total:\s*(\d+), heap peak:\s*(\d+)',(folder/'tools-200-alloc.log').read_text());assert m
 summary['allocated_bytes'],summary['peak_heap_bytes']=map(int,m.groups())
 summary['disclosed_median_us']=statistics.median(x for s in samples for x in s['samples_us'][::2])
 summary['collapsed_median_us']=statistics.median(x for s in samples for x in s['samples_us'][1::2])
 summaries[side]=summary
limits=json.loads(Path('/tmp/tui-tool-paint/acceptance-before-implementation.json').read_text())['limits']
checks={k:{'value':summaries['candidate'][k],'limit':limit,'pass':summaries['candidate'][k]<=limit} for k,limit in limits.items()}
content=[]
for sample in ['1','2','3','alloc']:
 reports={side:json.loads((out/side/f'tools-200-{sample}.json').read_text()) for side in sources}
 keys=['scenario','frames','history_turns','history_events','bytes','visible','oldest']
 for side in ['before','candidate']:
  for key in keys:assert reports[side][key]==reports['reference'][key],(side,sample,key)
 content.append({'sample':sample,'equal_for':list(sources),'fields':keys})
(out/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
(out/'comparison.json').write_text(json.dumps({'checks':checks,'content':content},indent=2)+'\n')
print(json.dumps(summaries,indent=2),flush=True)
assert all(c['pass'] for c in checks.values()),checks
