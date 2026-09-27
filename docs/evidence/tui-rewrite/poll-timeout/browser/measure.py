import pathlib, subprocess, json, statistics, time
root=pathlib.Path('/home/urbanbreach/Projects/agent-harness')
base=root/'.omo/evidence/tui-rewrite/poll-timeout'
binaries={'reference':'/home/urbanbreach/.codex/worktrees/tui-reference/agent-harness/target/release/examples/poll_timeout_rewrite_probe','candidate':'/tmp/tui-poll-timeout-rewrite-probe.bin'}
reports=[]
for repetition in range(1,4):
 for label in (('reference','candidate') if repetition !=2 else ('candidate','reference')):
  path=base/f'{label}-{repetition}';log=pathlib.Path(f'/tmp/tui-poll-timeout-browser-{label}-{repetition}.log')
  print(json.dumps({'started':label,'repetition':repetition,'at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())}),flush=True)
  with log.open('w') as output:
   subprocess.run(['node','scripts/qa/measure-rewrite-latency.mjs',binaries[label],str(path),'120'],cwd=root,stdout=output,stderr=subprocess.STDOUT,check=True)
  report=json.loads((path/'latency.json').read_text());reports.append({'build':label,'repetition':repetition,'path':str(path),'summary':report['summary'],'cleanup':report['cleanup'],'browser_cleanup':report['browser_cleanup'],'idle_output_bytes':report['idle_output_bytes']})
  print(json.dumps(reports[-1]),flush=True)
  (base/'paired-receipts.json').write_text(json.dumps(reports,indent=2)+'\n')
summary={label:{name:{percentile:statistics.median(report['summary'][name][percentile] for report in reports if report['build']==label) for percentile in ('p50_ms','p95_ms','p99_ms')} for name in reports[0]['summary']} for label in binaries}
(base/'paired-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary),flush=True)
for name in ('input','stream','resize'):
 for percentile in ('p95_ms','p99_ms'):
  assert summary['candidate'][name][percentile] <= summary['reference'][name][percentile]+16.7,(name,percentile,summary)
