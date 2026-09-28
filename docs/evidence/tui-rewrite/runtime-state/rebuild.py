from pathlib import Path
import hashlib,json,shutil,subprocess,sys
out=Path(__file__).resolve().parent
base='8c43e0319252c1afc6155c095eba742e0707eb98'
paths=[Path('crates/harness-tui/src')/name for name in ['view_model.rs','view_model/runtime.rs','app/lifecycle.rs','ui_chrome.rs','ui_live_turn_status.rs','runtime_integration.rs']]
current={p:p.read_bytes() if p.exists() else None for p in paths}
def build(side):
 with (out/f'build-{side}.log').open('w') as log:
  subprocess.run(['cargo','build','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--example','rewrite_probe'],stdout=log,stderr=log,check=True)
  metadata=json.loads(subprocess.check_output(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json'],stderr=log))
 binary=next(iter(metadata['rust-binaries'].values()));saved=out/f'{side}.bin';shutil.copy2(binary['binary-path'],saved);binary['binary-path']=str(saved)
 shutil.copy2('target/release/examples/rewrite_probe',out/f'{side}-probe')
 folder=out/side;folder.mkdir(exist_ok=True)
 (folder/'binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
 receipt={'base':base,'side':side,'sha256':hashlib.sha256(saved.read_bytes()).hexdigest(),'probe_sha256':hashlib.sha256((out/f'{side}-probe').read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(Path('crates/harness-tui/tests/rewrite_performance_test.rs').read_bytes()).hexdigest(),'production_sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None,'deleted':not p.exists()} for p in paths]}
 (folder/'binary.json').write_text(json.dumps(receipt,indent=2)+'\n')
if '--candidate-only' not in sys.argv:
 try:
  for p in paths:
   if subprocess.run(['git','cat-file','-e',f'{base}:{p}'],stderr=subprocess.DEVNULL).returncode == 0:
    p.write_bytes(subprocess.check_output(['git','show',f'{base}:{p}']))
   else:p.unlink(missing_ok=True)
  build('before')
 finally:
  for p,data in current.items():
   if data is None:p.unlink(missing_ok=True)
   else:p.write_bytes(data)
build('candidate')
(out/'cargo.json').write_bytes(subprocess.check_output(['cargo','metadata','--format-version','1']))
