from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/src/app/composer_editing.rs')
current=p.read_bytes()
old=subprocess.check_output(['git','show','172b776e6219c400005a43bd67d1f62e46099b48:'+str(p)]).decode()
assert old.count('line_end += 1;')==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','production_composer_reachability_test']
try:
 p.write_text(old.replace('line_end += 1;','line_end += 0;'))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log: result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 assert result.returncode==100
finally: p.write_bytes(current)
