from pathlib import Path
import json,shutil,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/tests/rewrite_viewport_probe.rs')
assert not p.exists(),p
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','rewrite_viewport_probe','--success-output','immediate']
try:
 shutil.copy2(out/'differential.rs',p)
 with (out/'oracle.log').open('w') as log: result=subprocess.run(command,stdout=log,stderr=log)
 (out/'oracle.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 result.check_returncode()
finally:p.unlink()
