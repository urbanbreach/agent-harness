from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/src/ui_composer/viewport.rs')
paths=[p,Path('crates/harness-tui/src/ui_composer.rs')]
current={path:path.read_bytes() for path in paths}
base='9681f5322d479a9d819af7fac09e5dfbfa053350'
source=subprocess.check_output(['git','show',f'{base}:{p}']).decode()
assert source.count('.filter(|break_at| *break_at > start)')==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','production_composer_reachability_test']
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show',f'{base}:{path}']))
 p.write_text(source.replace('.filter(|break_at| *break_at > start)', '.filter(|break_at| *break_at >= start)'))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 assert result.returncode==100
finally:
 for path,data in current.items():path.write_bytes(data)
