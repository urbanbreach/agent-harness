from pathlib import Path
import hashlib,json,shutil,subprocess
out=Path(__file__).resolve().parent
command=['cargo','build','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--example','rewrite_probe']
with (out/'build-candidate.log').open('w') as log:
 subprocess.run(command,stdout=log,stderr=log,check=True)
 metadata=json.loads(subprocess.check_output(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json'],stderr=log))
binary=next(iter(metadata['rust-binaries'].values()));saved=out/'candidate.bin';shutil.copy2(binary['binary-path'],saved);binary['binary-path']=str(saved)
shutil.copy2('target/release/examples/rewrite_probe',out/'candidate-probe')
folder=out/'candidate';folder.mkdir(exist_ok=True)
(folder/'binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
paths=[Path('crates/harness-tui/src')/name for name in ['layout.rs','layout/session.rs','layout/surfaces.rs','layout/overlays.rs']]
receipt={'base':json.loads((out/'base.json').read_text())['base'],'side':'candidate','command':command,'sha256':hashlib.sha256(saved.read_bytes()).hexdigest(),'probe_sha256':hashlib.sha256((out/'candidate-probe').read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(Path('crates/harness-tui/tests/rewrite_performance_test.rs').read_bytes()).hexdigest(),'production_sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'lines':len(p.read_text().splitlines())} for p in paths]}
(folder/'binary.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Candidate release/probe saved',receipt['sha256'])
