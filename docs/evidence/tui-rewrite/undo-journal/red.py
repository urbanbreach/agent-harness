from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/src/composer_editing/undo.rs')
paths=[Path(name) for name in ['crates/harness-tui/src/composer_atoms/buffer.rs','crates/harness-tui/src/composer_atoms/mod.rs','crates/harness-tui/src/composer_editing/undo.rs','crates/harness-tui/src/composer_editing/mod.rs']]
current={path:path.read_bytes() for path in paths}
source=subprocess.check_output(['git','show','68dd6c27462ed2435259b441bb06cf01eff7fd99:'+str(p)]).decode()
old='''            before: current,
            after: Arc::clone(&entry.after),'''
assert source.count(old)==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--test','composer_editing_test']
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show','68dd6c27462ed2435259b441bb06cf01eff7fd99:'+str(path)]))
 p.write_text(source.replace(old,'''            before: Arc::clone(&entry.after),
            after: Arc::clone(&entry.after),'''))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log: result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 assert result.returncode==100
finally:
 for path,data in current.items():path.write_bytes(data)
