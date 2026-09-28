from pathlib import Path
import datetime,json,subprocess
out=Path(__file__).resolve().parent;rows=[]
for name,command,required in [
 ('oracle',['python3',str(out/'oracle.py'),'oracle'],True),
 ('build',['python3',str(out/'build.py')],True),
 ('measure',['python3',str(out/'measure.py'),'before','candidate'],True),
 ('compare',['python3',str(out/'compare.py')],False),
 ('browser',['python3',str(out/'browser.py')],True),
]:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (out/f'{name}-run.log').open('w') as log:r=subprocess.run(command,stdout=log,stderr=log)
 rows.append({'name':name,'command':command,'started_at':start,'exit':r.returncode});(out/'validation.json').write_text(json.dumps(rows,indent=2)+'\n');print(name,r.returncode,flush=True)
 if required:r.check_returncode()
