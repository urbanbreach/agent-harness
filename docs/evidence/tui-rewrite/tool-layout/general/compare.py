from pathlib import Path
import json
root=Path('/tmp/tui-tool-layout/general')
summaries={side:json.loads((root/side/'summary.json').read_text())['scenarios'] for side in ('reference','before','candidate')}
(root/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
checks={}
for name in ['frozen-timing-comparison.json','frozen-resource-comparison.json']:
 d=json.loads((Path('docs/evidence/tui-rewrite/surface-painter/performance')/name).read_text())
 for scenario,metrics in d['metrics'].items():
  for metric,record in metrics.items():
   record['candidate']=summaries['candidate'][scenario][metric]
   record['passes']=record['candidate']<=record['limit']
   checks[scenario+':'+metric]=record['passes']
 (root/name).write_text(json.dumps(d,indent=2)+'\n')
reports={}
for p in (root/'before').glob('*.json'):
 if not p.name.startswith(('startup-','idle-','typing-','stream-','scroll-','resize-')):continue
 a=json.loads(p.read_text());b=json.loads((root/'candidate'/p.name).read_text());original=json.loads((root/'reference'/p.name).read_text())
 keys=['bytes','oldest','visible','frames','history_events','history_turns']
 for key in keys: assert a[key]==b[key],(p.name,key)
 reports[p.name]={'before_candidate_equal':True,'fields':keys,'original_differences':[key for key in keys if b[key]!=original[key]]}
(root/'content-comparison.json').write_text(json.dumps(reports,indent=2)+'\n')
order=json.loads((root/'run-order.json').read_text());assert len(order)==72 and all(d['exit_code']==0 for d in order)
print(checks,flush=True)
assert all(checks.values())
print('All 14 existing general workload limits pass; 24 before/candidate output pairs match.')
