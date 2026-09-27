from pathlib import Path
import json,os,subprocess,shutil
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-reader-wake')
checks=[
 ('build',['cargo','build','--release','-p','harness-tui','--all-features','--example','rewrite_probe','--example','resource_probe']),
 ('restoration',['python3','scripts/check-tui-restoration.py','--binary',str(out/'resource_probe'),'--live-binary',str(out/'rewrite_probe'),'--output',str(out/'green')]),
 ('all',['cargo','nextest','run','--profile','ci','--workspace','--all-features']),
 ('pty',['cargo','nextest','run','--profile','ci','-p','harness-tui','--all-features','--ignore-default-filter','-E','binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)']),
 ('clippy',['cargo','clippy','--workspace','--all-targets','--all-features','--','-D','warnings']),
 ('fmt',['cargo','fmt','--all','--','--check']),
 ('gates',['python3','scripts/check-test-suite-gates.py']),
]
results=[]
for name,command in checks:
 env=dict(os.environ)
 if name=='pty':env['HARNESS_TUI_PTY_SIGNOFF']='1'
 with (out/(name+'.log')).open('w') as log:r=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':command,'exit_code':r.returncode});(out/'checks.json').write_text(json.dumps(results,indent=2)+'\n')
 print(name,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
 if name=='build':
  for probe in ('rewrite_probe','resource_probe'):shutil.copy2(root/'target/release/examples'/probe,out/probe)
