from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('/home/urbanbreach/Projects/agent-harness');out=Path('/tmp/tui-tool-layout')
probe=out/'rewrite_probe';shutil.copy2(root/'target/release/examples/rewrite_probe',probe)
(out/'probe-binaries.json').write_text(json.dumps({side:{'path':str(path),'sha256':hashlib.sha256(path.read_bytes()).hexdigest()} for side,path in [('before',Path('/tmp/tui-tool-assembly-rewrite_probe.bin')),('candidate',probe)]},indent=2)+'\n')
commands=[
 ('final-rows',['python3',str(out/'check-rows.py')]),
 ('selection-before',['node','scripts/qa/capture-rewrite-selection.mjs','/tmp/tui-tool-assembly-rewrite_probe.bin','.omo/evidence/tui-rewrite/tool-layout/selection-before']),
 ('selection-candidate',['node','scripts/qa/capture-rewrite-selection.mjs',str(probe),'.omo/evidence/tui-rewrite/tool-layout/selection-candidate']),
 ('measure',['python3',str(out/'measure.py')]),
 ('measure-general',['python3',str(out/'measure-general.py')]),
 ('compare-general',['python3',str(out/'compare-general.py')]),
]
checks=[]
for name,command in commands:
 with (out/(name+'.log')).open('w') as log:r=subprocess.run(command,cwd=root,stdout=log,stderr=subprocess.STDOUT)
 checks.append({'name':name,'command':command,'exit_code':r.returncode});(out/'final-checks.json').write_text(json.dumps(checks,indent=2)+'\n')
 print(name,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
