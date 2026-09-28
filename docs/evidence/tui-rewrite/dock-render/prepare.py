from pathlib import Path
import subprocess,hashlib,json,shutil
out=Path(__file__).resolve().parent;prior=Path('/tmp/tui-frame-layout')
base=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip();assert base=='ec2fa068a7658c31ef033540918c4d525366ac94';assert not subprocess.check_output(['git','status','--porcelain'])
receipt=json.loads((prior/'candidate/binary.json').read_text())
for r in receipt['production_sources']:assert hashlib.sha256(Path(r['path']).read_bytes()).hexdigest()==r['sha256']
source_paths=set(subprocess.check_output(['git','diff','--name-only',receipt['base'],base,'--','crates/harness-tui/src']).decode().splitlines())
assert source_paths=={r['path'] for r in json.loads((prior/'sources.json').read_text())['files']}
assert hashlib.sha256(Path('crates/harness-tui/tests/rewrite_performance_test.rs').read_bytes()).hexdigest()==receipt['fixture_sha256']
for origin,destination,digest in [('candidate.bin','before.bin',receipt['sha256']),('candidate-probe','before-probe',receipt['probe_sha256'])]:
 assert hashlib.sha256((prior/origin).read_bytes()).hexdigest()==digest
 shutil.copy2(prior/origin,out/destination)
folder=out/'before';folder.mkdir(exist_ok=True)
metadata=json.loads((prior/'candidate/binaries.json').read_text())
for binary in metadata['rust-binaries'].values():binary['binary-path']=str(out/'before.bin')
(folder/'binaries.json').write_text(json.dumps(metadata,indent=2)+'\n');shutil.copy2(prior/'cargo.json',out/'cargo.json')
compiled_base=receipt['base']
receipt.update({'base':base,'side':'before','reuse_origin':str(prior/'candidate/binary.json')})
(folder/'binary.json').write_text(json.dumps(receipt,indent=2)+'\n')
(out/'before-reuse-origin.json').write_text(json.dumps({'source_base':base,'compiled_base':compiled_base,'verified_current_source_paths':sorted(source_paths),'previous_receipt':json.loads((prior/'candidate/binary.json').read_text())},indent=2)+'\n')
paths=[Path('crates/harness-tui/src')/name for name in ['ui_chrome.rs','ui_control_dock_disclosure.rs','ui_composer/bordered.rs','ui_composer/collapsed.rs','app/session_projection.rs','view_model.rs']]
rows=[]
for p in paths:
 data=p.read_bytes();assert data==subprocess.check_output(['git','show',f'{base}:{p}'])
 rows.append({'path':str(p),'sha256':hashlib.sha256(data).hexdigest(),'lines':len(data.splitlines())})
 target=out/'legacy'/p.relative_to('crates/harness-tui/src');target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(data)
(out/'base.json').write_text(json.dumps({'base':base,'source_tree':subprocess.check_output(['git','rev-parse','HEAD:crates/harness-tui/src']).decode().strip(),'sources':rows},indent=2)+'\n')
for name in ['measure.py','compare.py','checks.py']:shutil.copy2(prior/name,out/name)
print('Verified executable/fixture/source reuse; frozen dock sources recorded.')
