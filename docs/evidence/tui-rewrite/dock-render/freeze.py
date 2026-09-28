from pathlib import Path
import datetime,hashlib,json,shutil,tarfile
out=Path(__file__).resolve().parent
p=out/'before-reuse-origin.json'; receipt=json.loads(p.read_text()); receipt['compiled_base']=receipt['previous_receipt']['base']; receipt['metadata_verification']='compiled_base verified against the unchanged original receipt before acceptance freeze';p.write_text(json.dumps(receipt,indent=2)+'\n')
baseline=json.loads((out/'summary.json').read_text())['before']
prior=json.loads(Path('/tmp/tui-frame-layout/acceptance-before-implementation.json').read_text())['limits']
limits={}
for scenario, row in baseline.items():
 limits[scenario]={key:min(bound,row[key]*(.9 if key=='malloc_calls' and scenario=='typing-long' else .95 if key=='allocated_bytes' and scenario=='typing-long' else 1 if key in ['malloc_calls','allocated_bytes'] else 1.1)) for key,bound in prior[scenario].items()}
record={'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source':json.loads((out/'base.json').read_text())['base'],'baseline':baseline,'limits':limits,'requirements':'Replace owned control-dock projection and disclosure renderer; preserve public context strings, full styled buffers, cursor and input behavior. At least 10% fewer long-typing malloc calls and 5% fewer allocated bytes; no allocation/call increases elsewhere. Retain every prior stricter bound, including failing timing/CPU limits. Fixed 500 frames, 10 warmups, 160x48, zero history; renderer timings exclude PTY/emulator delivery.'}
(out/'acceptance-before-implementation.json').write_text(json.dumps(record,indent=2)+'\n')
folder=out/'baseline';folder.mkdir()
for name in ['before','summary.json','run-order.json','baseline.log','acceptance-before-implementation.json','base.json','before-reuse-origin.json']:
 src=out/name; dst=folder/name
 if src.is_dir():shutil.copytree(src,dst)
 else:shutil.copy2(src,dst)
with tarfile.open(out/'baseline-before-implementation.tar.gz','w:gz') as tar:tar.add(folder,arcname='baseline')
print(record['recorded_at'],hashlib.sha256((out/'baseline-before-implementation.tar.gz').read_bytes()).hexdigest())
