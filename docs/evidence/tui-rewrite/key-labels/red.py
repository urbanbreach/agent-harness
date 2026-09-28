from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent
p=Path('crates/harness-tui/src/keybindings.rs');current=p.read_bytes()
source=(out/'legacy-keybindings.rs').read_text()
control='    if binding.modifiers.contains(KeyModifiers::CONTROL) {\n        parts.push("Ctrl");\n    }\n'
shift='    if binding.modifiers.contains(KeyModifiers::SHIFT) {\n        parts.push("Shift");\n    }\n'
assert source.count(control+shift)==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','-E','test(keybindings::tests)']
try:
 p.write_text(source.replace(control+shift,shift+control))
 (out/'red.patch').write_bytes(subprocess.check_output(['git','diff','--',str(p)]))
 with (out/'red.log').open('w') as log:result=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'command':command,'exit':result.returncode},indent=2)+'\n')
 assert result.returncode==100
finally:p.write_bytes(current)
