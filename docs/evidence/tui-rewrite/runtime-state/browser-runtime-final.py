from pathlib import Path
import subprocess,json,datetime
out=Path(__file__).resolve().parent
root=Path('.omo/evidence/tui-rewrite/runtime-state')
results=[]
for journey,script in [('runtime','capture-runtime.mjs')]:
 for side in ['before','candidate']:
  command=['node',str(out/script),str(out/f'{side}-probe'),str(root/journey/side)]
  start=datetime.datetime.now(datetime.timezone.utc).isoformat()
  with (out/f'capture-{journey}-{side}.log').open('w') as log:
   result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
  results.append({'journey':journey,'side':side,'started_at':start,'command':command,'exit':result.returncode})
  (out/'runtime-final-runs.json').write_text(json.dumps(results,indent=2)+'\n');print(journey,side,result.returncode,flush=True);result.check_returncode()
 command=['python3',str(out/('compare-browser.py' if journey=='composer' else f'compare-{journey}.py')),str(root/journey),str(out/f'browser-{journey}-comparison.json')]
 with (out/f'compare-{journey}.log').open('w') as log:
  result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 print('compare',journey,result.returncode,flush=True);result.check_returncode()
