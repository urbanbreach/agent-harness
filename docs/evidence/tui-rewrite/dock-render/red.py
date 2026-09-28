from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent;p=Path('crates/harness-tui/src/ui_control_dock_disclosure.rs');original=p.read_bytes()
old=b'summary_idx + (hint_idx * 2)';new=b'hint_idx + (summary_idx * 2)';assert original.count(old)==2
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--lib','-E','test(live_composer_disclosure_keeps_compact_summary_and_commands)']
try:
 p.write_bytes(original.replace(old,new))
 with (out/'red.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'mutation':'Reverse weighted summary/hint priority in both selection branches','command':command,'exit':r.returncode},indent=2)+'\n')
 assert r.returncode==100
 assert 'disclosure priority' in (out/'red.log').read_text()
finally:p.write_bytes(original)
