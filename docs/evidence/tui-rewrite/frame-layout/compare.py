from pathlib import Path
import json
out=Path(__file__).resolve().parent
summary=json.loads((out/'summary.json').read_text())
limits=json.loads((out/'acceptance-before-implementation.json').read_text())['limits']
checks={name:{key:{'value':summary['candidate'][name][key],'limit':bound,'pass':summary['candidate'][name][key]<=bound} for key,bound in values.items()} for name,values in limits.items()}
content=[]
for scenario in limits:
 for sample in ['1','2','3','alloc']:
  a=json.loads((out/'before'/f'{scenario}-{sample}.json').read_text());b=json.loads((out/'candidate'/f'{scenario}-{sample}.json').read_text())
  fields=['scenario','history_turns','history_events','frames','bytes','visible','oldest']
  assert all(a[key]==b[key] for key in fields),(scenario,sample)
  content.append({'scenario':scenario,'sample':sample,'equal_fields':fields})
(out/'comparison.json').write_text(json.dumps({'checks':checks,'content':content},indent=2)+'\n')
assert all(check['pass'] for group in checks.values() for check in group.values()),checks
print(f'All {sum(map(len, checks.values()))} frozen limits and paired content checks pass.')
