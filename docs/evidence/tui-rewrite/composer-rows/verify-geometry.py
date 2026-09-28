from pathlib import Path
import hashlib,json,subprocess,sys
out=Path(__file__).resolve().parent
paths=[Path('crates/harness-tui/tests')/name for name in ['rewrite_viewport_probe.rs','rewrite_row_probe.rs']]
assert not any(p.exists() for p in paths),paths
command=['cargo', 'nextest', 'run', '--profile', 'ci', '-p', 'harness-tui', '--all-features', '--test', 'rewrite_viewport_probe', '--test', 'rewrite_row_probe', '-E', 'test(recorded_)', '--success-output', 'immediate']
try:
 subprocess.run([sys.executable,str(out/'make-oracles.py')],check=True)
 subprocess.run(['cargo','fmt','--all'],check=True)
 fixtures=[{'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in paths]
 with (out/'oracle.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'oracle.json').write_text(json.dumps({'command':command,'exit':result.returncode,'fixtures':fixtures},indent=2)+'\n')
 result.check_returncode()
finally:
 for p in paths:p.unlink(missing_ok=True)
