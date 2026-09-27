from pathlib import Path
import hashlib,json
root=Path('/tmp/tui-tool-assembly-performance')
summaries={side:json.loads((root/side/'summary.json').read_text()) for side in ['reference','before','candidate']}
limits=json.loads((root/'acceptance-before-implementation.json').read_text())['candidate_limits']
checks={key:{'candidate':summaries['candidate'][key],'limit':limit,'passes':summaries['candidate'][key]<=limit} for key,limit in limits.items()}
content=[]
for sample in ['1','2','3','alloc']:
 reports={side:json.loads((root/side/f'tools-200-{sample}.json').read_text()) for side in summaries}
 keys=['scenario','frames','history_turns','history_events','bytes','visible','oldest']
 for side in ['reference','candidate']:
  for key in keys:assert reports[side][key]==reports['before'][key],(side,sample,key)
 content.append({'sample':sample,'equal_for':['reference','before','candidate'],'checked':keys})
for side in summaries:
 runs=json.loads((root/side/'run-order.json').read_text());assert len(runs)==4 and all(run['exit_code']==0 for run in runs)
for kind,record in json.loads(Path('docs/evidence/tui-rewrite/edit-owner/frames/comparison.json').read_text()).items():
 for name,digest in record['files'].items():
  assert hashlib.sha256((Path('/tmp/tui-tool-assembly-'+kind)/name).read_bytes()).hexdigest()==digest,(kind,name)
(root/'summary.json').write_text(json.dumps(summaries,indent=2)+'\n')
(root/'comparison.json').write_text(json.dumps({'acceptance':checks,'content':content,'all_limits_pass':all(check['passes'] for check in checks.values())},indent=2)+'\n')
assert all(check['passes'] for check in checks.values()),checks
print('All 6 frozen limits pass; all 12 tool runs have equal output/count receipts across the three builds. Final-source frame hashes match.')
print(json.dumps(summaries,indent=2))
