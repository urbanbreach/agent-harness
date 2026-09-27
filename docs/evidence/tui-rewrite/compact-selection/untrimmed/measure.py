from pathlib import Path
import copy,datetime,hashlib,json,os,re,statistics,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-compact-selection-performance');out.mkdir(exist_ok=True)
metadata=json.loads(Path('/tmp/tui-compact-selection-binaries.json').read_text());cargo='/tmp/tui-compact-selection-cargo.json'
binaries={'reference':'/tmp/tui-styled-wrap-reference.bin','before':'/tmp/tui-styled-wrap-candidate.bin','candidate':'/tmp/tui-compact-selection-candidate.bin'}
commits={'reference':'1bb0f98988670a5f4b48cdf749b455a79cfdaa82','before':'8e7a3ca0','candidate':'8b01eed0 + source.patch.gz'}
commands={};summaries={}
for side,path in binaries.items():
 folder=out/side;folder.mkdir(exist_ok=True);m=copy.deepcopy(metadata)
 next(iter(m['rust-binaries'].values()))['binary-path']=path
 (folder/'nextest-binaries.json').write_text(json.dumps(m,indent=2)+'\n')
 commands[side]=['cargo','nextest','run','--binaries-metadata',str(folder/'nextest-binaries.json'),'--cargo-metadata',cargo,'--profile','perf','-j1','--success-output','immediate']
 receipt={'path':path,'bytes':Path(path).stat().st_size,'sha256':hashlib.sha256(Path(path).read_bytes()).hexdigest(),'source':commits[side]}
 (folder/'binary.json').write_text(json.dumps(receipt,indent=2)+'\n')
 summaries[side]={'source':commits[side],'binary_sha256':receipt['sha256'],'command':commands[side],'scenarios':{}}
order=[]
workloads=[('startup',0),('idle',1000),('typing',1000),('stream',1000),('scroll',1000),('resize',1000)]
for allocation in [False,True]:
 for scenario,history in workloads:
  for repetition in range(1,2 if allocation else 4):
   for side in (['reference','before','candidate'] if repetition%2 else ['candidate','before','reference']):
    stem=f'{scenario}-{history}-'+('alloc' if allocation else str(repetition));folder=out/side
    env={**os.environ,'HARNESS_REWRITE_SCENARIO':scenario,'HARNESS_REWRITE_HISTORY':str(history),'HARNESS_REWRITE_FRAMES':'200','HARNESS_REWRITE_PERF_OUT':str(folder/(stem+'.json'))}
    command=commands[side]
    if allocation:command=['memusage','-n',Path(binaries[side]).name,'--no-timer',*command]
    entry={'side':side,'run':stem,'allocation':allocation,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command}
    with (folder/(stem+'.log')).open('w') as log:r=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
    entry['exit_code']=r.returncode;order.append(entry);(out/'run-order.json').write_text(json.dumps(order,indent=2)+'\n')
    if r.returncode:raise SystemExit(f'{side} {stem} failed {r.returncode}')
  print(scenario,'allocation' if allocation else 'timing','finished',flush=True)
for side in binaries:
 folder=out/side
 for scenario,history in workloads:
  stem=f'{scenario}-{history}';samples=[json.loads((folder/f'{stem}-{rep}.json').read_text()) for rep in range(1,4)]
  fields=['construction_us','cold_us','p50_us','p95_us','p99_us','bytes']
  summary={key:statistics.median(sample[key] for sample in samples) for key in fields}
  summary['rss_kib']=statistics.median(sample['after']['rss_kib'] for sample in samples)
  summary['cpu_ms_per_frame']=statistics.median((s['after']['cpu_ticks']-s['before']['cpu_ticks'])*1000/os.sysconf('SC_CLK_TCK')/200 for s in samples)
  m=re.search(r'heap total:\s*(\d+), heap peak:\s*(\d+)',(folder/f'{stem}-alloc.log').read_text());assert m,stem
  summary['allocated_bytes'],summary['peak_heap_bytes']=map(int,m.groups())
  summaries[side]['scenarios'][stem]=summary
 (folder/'summary.json').write_text(json.dumps(summaries[side],indent=2)+'\n')
print(json.dumps({s:r['scenarios'] for s,r in summaries.items()},indent=2),flush=True)
