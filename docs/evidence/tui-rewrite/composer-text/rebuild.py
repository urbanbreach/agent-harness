from pathlib import Path
import hashlib,json,shutil,subprocess
out=Path(__file__).resolve().parent
base='92bd235d0d8a70e84ee3a7f333fcbb7a10f8aa30'
paths=[Path(p) for p in ['crates/harness-tui/src/app.rs','crates/harness-tui/src/composer_atoms/buffer.rs','crates/harness-tui/src/composer_integration/controllers.rs']]
current={p:p.read_bytes() for p in paths}
shutil.copy2('target/release/examples/rewrite_probe',out/'rewrite_probe')
(out/'source.patch').write_bytes(subprocess.check_output(['git','diff','--binary',base,'--','crates/harness-tui/src']))
def build(side):
    command=['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json']
    with (out/f'build-{side}-guarded.log').open('w') as log:
        raw=subprocess.check_output(command,stderr=log)
    metadata=json.loads(raw)
    binary=next(iter(metadata['rust-binaries'].values()))
    kept=out/f'{side}.bin'
    shutil.copy2(binary['binary-path'],kept)
    binary['binary-path']=str(kept)
    folder=out/side;folder.mkdir(exist_ok=True)
    (folder/'binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
    (folder/'binary.json').write_text(json.dumps({'source':base + (' plus source.patch' if side=='candidate' else ''),'fixture_sha256':hashlib.sha256(Path('crates/harness-tui/tests/rewrite_performance_test.rs').read_bytes()).hexdigest(),'sha256':hashlib.sha256(kept.read_bytes()).hexdigest(),'production_sources':[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]},indent=2)+'\n')
    print(side,'guarded fixture built',flush=True)
try:
    for p in paths:
        p.write_bytes(subprocess.check_output(['git','show',f'{base}:{p}']))
    build('before')
finally:
    for p,data in current.items():
        p.write_bytes(data)
build('candidate')
