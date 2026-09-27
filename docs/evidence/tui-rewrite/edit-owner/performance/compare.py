import json,pathlib
root=pathlib.Path('/tmp/tui-edit-owner-performance')
summaries={side:json.loads((root/side/'summary.json').read_text())['scenarios'] for side in ('reference','before','candidate')}
(root/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
for name in ['frozen-timing-comparison.json','frozen-resource-comparison.json']:
 d=json.loads((pathlib.Path('docs/evidence/tui-rewrite/surface-painter/performance')/name).read_text())
 for scenario,metrics in d['metrics'].items():
  for metric,record in metrics.items():
   record['candidate']=summaries['candidate'][scenario][metric]
   record['passes']=record['candidate']<=record['limit']
   assert record['passes'],(scenario,metric,record)
 (root/name).write_text(json.dumps(d,indent=2)+'\n')
limits=json.load(open('/tmp/tui-settled-acceptance.json'))['candidate_limits']
d={k:{'candidate':summaries['candidate']['settle-1000'][k],'limit':v,'passes':summaries['candidate']['settle-1000'][k]<=v} for k,v in limits.items()}
assert all(v['passes'] for v in d.values())
(root/'settlement-acceptance.json').write_text(json.dumps(d,indent=2)+'\n')
reports={}
for p in (root/'before').glob('*.json'):
 if not p.name.startswith(('settle-','startup-','idle-','typing-','stream-','scroll-','resize-')):continue
 a=json.loads(p.read_text());b=json.loads((root/'candidate'/p.name).read_text())
 for key in ['bytes','oldest','visible','frames','history_events','history_turns']:
  assert a[key]==b[key],(p.name,key)
  if p.name.startswith('settle-'):assert a[key]==json.loads((root/'reference'/p.name).read_text())[key],('original',p.name,key)
 reports[p.name]=True
(root/'content-comparison.json').write_text(json.dumps(reports,indent=2)+'\n')
order=json.loads((root/'run-order.json').read_text());assert len(order)==84 and all(d['exit_code']==0 for d in order)
print('All 19 frozen checks pass; 28 before/candidate output pairs match; original settlement output matches.')
print(json.dumps({side:summaries[side]['settle-1000'] for side in summaries},indent=2))
