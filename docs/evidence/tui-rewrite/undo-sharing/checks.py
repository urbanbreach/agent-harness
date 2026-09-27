from pathlib import Path
import json,os,subprocess
out=Path(__file__).resolve().parent
checks=[
('workspace-check',['cargo','check','--workspace'],{}),
('fmt',['cargo','fmt','--all','--','--check'],{}),
('suite-gates',['python3','scripts/check-test-suite-gates.py'],{}),
('pty',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--ignore-default-filter','-E','binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'],{'HARNESS_TUI_PTY_SIGNOFF':'1'})]
results=[]
for name,command,env in checks:
    with (out/f'{name}.log').open('w') as log:
        result=subprocess.run(command,env={**os.environ,**env},stdout=log,stderr=subprocess.STDOUT)
    results.append({'name':name,'command':command,'environment':env,'exit':result.returncode})
    (out/'checks.json').write_text(json.dumps(results,indent=2)+'\n')
    print(name,result.returncode,flush=True)
    result.check_returncode()
with (out/'build-release.log').open('w') as log:
    subprocess.run(['cargo','build','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--example','rewrite_probe'],stdout=log,stderr=log,check=True)
with (out/'candidate-binaries.json').open('w') as output, (out/'list-release.log').open('w') as log:
    subprocess.run(['cargo','nextest','list','--release','-p','harness-tui','--all-features','--test','rewrite_performance_test','--list-type','binaries-only','--message-format','json'],stdout=output,stderr=log,check=True)
