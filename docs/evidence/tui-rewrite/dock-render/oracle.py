from pathlib import Path
import gzip,hashlib,json,os,subprocess,sys
out=Path(__file__).resolve().parent;side=sys.argv[1];assert side in ['before','candidate']
p=Path('crates/harness-tui/src/app.rs');original=p.read_bytes()
command=['cargo','nextest','run','--config-file',str(out/'nextest-oracle.toml'),'--profile','ci','-p','harness-tui','--lib','-E','test(dock_render_reference)','--success-output','immediate']
try:
 p.write_bytes(original+f'\n#[cfg(test)]\n#[path = "{out}/oracle.rs"]\nmod frozen_dock_render;\n'.encode())
 raw=out/f'oracle-{side}.jsonl'
 with (out/f'oracle-{side}.log').open('w') as log:r=subprocess.run(command,env={**os.environ,'HARNESS_DOCK_ORACLE_OUT':str(raw)},stdout=log,stderr=log)
 (out/f'oracle-{side}-run.json').write_text(json.dumps({'command':command,'exit':r.returncode,'oracle_sha256':hashlib.sha256((out/'oracle.rs').read_bytes()).hexdigest()},indent=2)+'\n');r.check_returncode()
 with raw.open('rb') as f,gzip.open(str(raw)+'.gz','wb') as g:
  import shutil;shutil.copyfileobj(f,g)
 print(side,'oracle',raw.stat().st_size,flush=True)
 if side=='candidate':
  count=0
  with (out/'oracle-before.jsonl').open() as a,raw.open() as b:
   import itertools
   for i,(before,after) in enumerate(itertools.zip_longest(a,b)):
    assert before==after, f'oracle case {i} differs'
    count+=1
  (out/'oracle-comparison.json').write_text(json.dumps({'cases':count,'exact_buffers_cursor_context_equal':True},indent=2)+'\n')
finally:p.write_bytes(original)
