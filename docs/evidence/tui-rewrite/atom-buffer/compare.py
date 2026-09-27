from pathlib import Path
import json
out=Path(__file__).resolve().parent
summary=json.loads((out/'summary.json').read_text())
limits=json.loads((out/'acceptance-before-implementation.json').read_text())['limits']
earlier=json.loads((Path.cwd()/'docs/evidence/tui-rewrite/composer-text/acceptance-before-implementation.json').read_text())['limits']
for name, values in earlier.items():
    for key, bound in values.items():
        limits[name][key]=min(limits[name][key],bound)
limits['typing-long']['cpu_ms_per_frame']=min(limits['typing-long']['cpu_ms_per_frame'],json.loads((out/'acceptance-second-step.json').read_text())['cpu_ms_per_frame_limit'])
checks={name:{key:{'value':summary['candidate'][name][key],'limit':value,'pass':summary['candidate'][name][key]<=value} for key,value in bound.items()} for name,bound in limits.items()}
content=[]
for scenario in ['typing-long','typing']:
    for sample in ['1','2','3','alloc']:
        a=json.loads((out/'before'/f'{scenario}-{sample}.json').read_text())
        b=json.loads((out/'candidate'/f'{scenario}-{sample}.json').read_text())
        fields=['scenario','history_turns','history_events','frames','bytes','visible','oldest']
        assert all(a[key]==b[key] for key in fields),(scenario,sample)
        content.append({'scenario':scenario,'sample':sample,'equal_fields':fields})
(out/'comparison.json').write_text(json.dumps({'checks':checks,'content':content},indent=2)+'\n')
assert all(check['pass'] for group in checks.values() for check in group.values()),checks
print('All frozen limits and before/candidate content checks pass.')
