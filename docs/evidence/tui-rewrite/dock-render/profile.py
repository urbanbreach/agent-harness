from pathlib import Path
import hashlib,json,os,shutil,subprocess
out=Path(__file__).resolve().parent
artifacts=[json.loads(line) for line in (out/'profile-build.jsonl').read_text().splitlines()]
artifact=next(x for x in artifacts if x.get('reason')=='compiler-artifact' and x['target']['name']=='rewrite_performance_test')
saved=out/'profile.bin';shutil.copy2(artifact['executable'],saved)
metadata=json.loads((out/'before/binaries.json').read_text());next(iter(metadata['rust-binaries'].values()))['binary-path']=str(saved)
(out/'profile-binaries.json').write_text(json.dumps(metadata,indent=2)+'\n')
command=['cargo','nextest','run','--config',f'target.x86_64-unknown-linux-gnu.runner="{out}/profile-runner.py"','--binaries-metadata',str(out/'profile-binaries.json'),'--cargo-metadata',str(out/'cargo.json'),'--profile','perf','-j1','--success-output','immediate']
env={'HARNESS_REWRITE_SCENARIO':'typing-long','HARNESS_REWRITE_HISTORY':'0','HARNESS_REWRITE_FRAMES':'500','HARNESS_REWRITE_PERF_OUT':str(out/'profile-workload.json')}
with (out/'profile-run.log').open('w') as log:r=subprocess.run(command,env={**os.environ,**env},stdout=log,stderr=log)
(out/'profile-command.json').write_text(json.dumps({'source':json.loads((out/'base.json').read_text())['base'],'purpose':'Diagnosis only, every 200 malloc hits including setup/warmup; excludes calloc/realloc','binary_sha256':hashlib.sha256(saved.read_bytes()).hexdigest(),'command':command,'environment':env,'exit':r.returncode},indent=2)+'\n');r.check_returncode()
subprocess.run(['python3',str(out/'profile-summary.py')],check=True)
