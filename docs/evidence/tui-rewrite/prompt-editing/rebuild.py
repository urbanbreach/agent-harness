from pathlib import Path
import hashlib,json,shutil,subprocess
out=Path(__file__).resolve().parent
base='172b776e6219c400005a43bd67d1f62e46099b48'
source=Path('crates/harness-tui/src/app/composer_editing.rs')
current=source.read_bytes()
def build(side):
 with (out/f'build-{side}.log').open('w') as log:
  subprocess.run(['cargo','build','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--example','rewrite_probe'],stdout=log,stderr=log,check=True)
  metadata=json.loads(subprocess.check_output(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json'],stderr=log))
 binary=next(iter(metadata['rust-binaries'].values())); saved=out/f'{side}.bin'; shutil.copy2(binary['binary-path'],saved); binary['binary-path']=str(saved)
 shutil.copy2('target/release/examples/rewrite_probe',out/f'{side}-probe')
 folder=out/side; folder.mkdir(exist_ok=True)
 (folder/'binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
 (folder/'binary.json').write_text(json.dumps({'base':base,'side':side,'sha256':hashlib.sha256(saved.read_bytes()).hexdigest(),'probe_sha256':hashlib.sha256((out/f'{side}-probe').read_bytes()).hexdigest(),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'fixture_sha256':hashlib.sha256(Path('crates/harness-tui/tests/rewrite_performance_test.rs').read_bytes()).hexdigest()},indent=2)+'\n')
try:
 source.write_bytes(subprocess.check_output(['git','show',f'{base}:{source}']))
 build('before')
finally:
 source.write_bytes(current)
build('candidate')
(out/'cargo.json').write_bytes(subprocess.check_output(['cargo','metadata','--format-version','1']))
