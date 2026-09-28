from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent;base='b832c3ca0afbe2ec71e473ac63c744f8abdc83a8'
paths=[Path(row['path']) for row in json.loads((out/'before/binary.json').read_text())['production_sources']]
current={p:p.read_bytes() for p in paths};p=Path('crates/harness-tui/src/composer_atoms/buffer.rs')
source=subprocess.check_output(['git','show',f'{base}:{p}']).decode();assert source.count('current.display_width > 0')==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','composer_atoms_test','--test','completion_controller_test','--test','production_composer_reachability_test']
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show',f'{base}:{path}']))
 p.write_text(source.replace('current.display_width > 0','!current.atom_ids.is_empty()'))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n');assert result.returncode==100
finally:
 for path,data in current.items():path.write_bytes(data)
