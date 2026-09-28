from pathlib import Path
import json,subprocess
out=Path(__file__).resolve().parent;root=Path('.omo/evidence/tui-rewrite/dock-render/dock');results=[]
commands=[(side,['node',str(out/'capture-dock.mjs'),str(out/f'{side}-probe'),str(root/side)]) for side in ['before','candidate']]+[('comparison',['python3',str(out/'compare-dock.py'),str(root),str(out/'browser-dock-comparison.json')])]
for name,command in commands:
 with (out/f'dock-final-{name}.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':command,'exit':r.returncode})
 (out/'dock-final-runs.json').write_text(json.dumps(results,indent=2)+'\n');print(name,r.returncode,flush=True);r.check_returncode()
