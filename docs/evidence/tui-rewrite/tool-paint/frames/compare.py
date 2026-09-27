from pathlib import Path
import json,hashlib
out=Path('/tmp/tui-tool-paint')
frozen=json.loads(Path('docs/evidence/tui-rewrite/tool-layout/frames/comparison.json').read_text());results={}
for name,record in frozen.items():
 base=Path('/tmp/tui-tool-paint-'+name);files=record['files']
 for rel,sha in files.items():assert hashlib.sha256((base/rel).read_bytes()).hexdigest()==sha,(name,rel)
 actual={str(p.relative_to(base)) for p in base.rglob('*') if p.is_file()}
 assert actual==set(files),(name,actual^set(files))
 results[name]={'equal':True,'baseline':'docs/evidence/tui-rewrite/tool-layout/frames/comparison.json','files':files,'candidate':str(base)}
for name in ['body','order','interactions']:
 a=out/('before-'+name);b=out/('candidate-'+name)
 files={str(p.relative_to(a)):hashlib.sha256(p.read_bytes()).hexdigest() for p in a.rglob('*') if p.is_file() and not p.name.endswith('producer.json')}
 others={str(p.relative_to(b)) for p in b.rglob('*') if p.is_file() and not p.name.endswith('producer.json')}
 assert set(files)==others
 for rel,sha in files.items():assert hashlib.sha256((b/rel).read_bytes()).hexdigest()==sha,(name,rel)
 results[name]={'equal':True,'before':str(a),'candidate':str(b),'files':files,'excluded':['producer.json metadata']}
(out/'frame-comparison.json').write_text(json.dumps(results,indent=2)+'\n')
print({k:len(v['files']) for k,v in results.items()})
