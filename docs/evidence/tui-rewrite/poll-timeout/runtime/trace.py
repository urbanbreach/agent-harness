import importlib.util,json,os,pathlib,collections
root=pathlib.Path('/home/urbanbreach/Projects/agent-harness')
s=importlib.util.spec_from_file_location('probe',root/'scripts/measure-tui-runtime.py');p=importlib.util.module_from_spec(s);s.loader.exec_module(p)
os.environ['LD_PRELOAD']='/tmp/tui_poll_trace.so';os.environ['HARNESS_PROBE_POLL_TRACE']='/tmp/tui-poll-timeout-runtime/poll-trace.txt'
r=p.measure(pathlib.Path('/tmp/tui-poll-timeout-resource-probe.bin'),'idle',2,None)
rows=[list(map(int,line.split())) for line in pathlib.Path(os.environ['HARNESS_PROBE_POLL_TRACE']).read_text().splitlines()]
data={'diagnostic_only':True,'resources':r,'poll_calls':len(rows),'timeout_counts':dict(collections.Counter(row[1] for row in rows)),'threads':dict(collections.Counter(row[0] for row in rows))}
pathlib.Path('/tmp/tui-poll-timeout-runtime/poll-summary.json').write_text(json.dumps(data,indent=2)+'\n');print(json.dumps(data))
