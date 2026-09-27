from pathlib import Path
import os,subprocess,json,shutil
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-focus');results=[]
def run(name,cmd,extra=None):
    with (out/(name+'.log')).open('w') as log:
        result=subprocess.run(cmd,cwd=root,env={**os.environ,**(extra or {})},stdout=log,stderr=subprocess.STDOUT)
    results.append({'name':name,'command':cmd,'env':extra or {},'exit_code':result.returncode})
    (out/'checks.json').write_text(json.dumps(results,indent=2)+'\n'); print(name,result.returncode,flush=True)
    if result.returncode: raise SystemExit(result.returncode)
key=root/'crates/harness-tui/src/app/key_interaction.rs';app=root/'crates/harness-tui/src/app.rs'
saved_key=key.read_bytes();saved_app=app.read_bytes()
try:
    key.write_bytes((out/'before-key_interaction.rs').read_bytes())
    app.write_bytes(saved_app.replace(b'mod focus;\n',b''))
    run('before-expanded',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','-E','test(focus_shortcuts_follow_visible_shell_and_preserve_replay) | test(help_preempts_palette_and_close_restores_original_focus) | binary(rewrite_reference_test)'], {'HARNESS_TUI_REFERENCE_FRAMES':str(out/'before-frames')})
finally:
    key.write_bytes(saved_key);app.write_bytes(saved_app)
run('candidate-expanded',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','-E','test(focus_shortcuts_follow_visible_shell_and_preserve_replay) | test(help_preempts_palette_and_close_restores_original_focus) | binary(rewrite_reference_test)'], {'HARNESS_TUI_REFERENCE_FRAMES':str(out/'candidate-frames')})
run('clippy',['cargo','clippy','-p','harness-tui','--all-targets','--all-features','--','-D','warnings'])
run('workspace',['cargo','check','--workspace'])
run('fmt',['cargo','fmt','--all','--','--check'])
run('gates',['python3','scripts/check-test-suite-gates.py'])
run('pty',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--ignore-default-filter','-E','binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'],{'HARNESS_TUI_PTY_SIGNOFF':'1'})
run('release',['cargo','build','--release','-p','harness-tui','--all-features','--example','rewrite_probe'])
shutil.copy2(root/'target/release/examples/rewrite_probe',out/'rewrite_probe')
