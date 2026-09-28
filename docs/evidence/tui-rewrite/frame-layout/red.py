from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent;p=Path('crates/harness-tui/src/layout.rs');original=p.read_bytes()
old=b'session_operator_overlay(body, contract)';new=b'session_operator_overlay(transcript, contract)';assert original.count(old)==1
command=['cargo','nextest','run','--profile','ci','-p','harness-tui','--lib','-E','test(hovered_wheel_target_uses_layout_plan) | test(question_mouse_click_preserves_shell_state_and_emits_only_answer_intent)']
try:
 p.write_bytes(original.replace(old,new))
 with (out/'red.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=log)
 (out/'red.json').write_text(json.dumps({'mutation':'Size details overlay after terminal split instead of before it','command':command,'exit':r.returncode},indent=2)+'\n')
 assert r.returncode==100
 assert 'assertion `left == right` failed' in (out/'red.log').read_text()
finally:p.write_bytes(original)
