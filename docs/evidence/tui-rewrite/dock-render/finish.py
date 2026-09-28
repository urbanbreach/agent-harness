from pathlib import Path
import json, subprocess
out=Path(__file__).resolve().parent
steps=[('measure',['python3',str(out/'measure.py'),'before','candidate']),
       ('compare',['python3',str(out/'compare.py')]),
       ('browser',['python3',str(out/'browser.py')]),
       ('workspace',['python3',str(out/'workspace.py')])]
results=[]
for name,command in steps:
 with (out/f'{name}-run.log').open('w') as log:
  result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':command,'exit':result.returncode})
 (out/'validation.json').write_text(json.dumps(results,indent=2)+'\n')
 print(name,result.returncode,flush=True)
 if name!='compare':result.check_returncode()
