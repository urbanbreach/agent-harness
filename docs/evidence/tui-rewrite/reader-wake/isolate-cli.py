from pathlib import Path
import json,os,re,subprocess,tempfile
out=Path('/tmp/tui-reader-wake');root=Path('/home/urbanbreach/Projects/agent-harness');results=[]
with tempfile.TemporaryDirectory(prefix='harness-reader-config-') as config:
 env={**os.environ,'XDG_CONFIG_HOME':config,'CARGO_TARGET_DIR':str(root/'target')}
 for label,path in [('before',out/'baseline-src'),('candidate',root)]:
  command=['cargo','nextest','run','--manifest-path',str(path/'Cargo.toml'),'--profile','ci','-p','harness','--all-features']
  log=out/f'cli-isolated-{label}.log'
  with log.open('w') as stream:r=subprocess.run(command,cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT)
  text=log.read_text();results.append(dict(build=label,command=command,exit_code=r.returncode,summary=re.findall(r'^.*Summary.*$',text,re.M),failures=sorted(set(re.findall(r'^\s*FAIL \[[^\]]+\] \([^)]*\) (.*)$',text,re.M)))))
  print(label,results[-1]['summary'],flush=True)
assert results[0]['failures']==results[1]['failures'],results
(out/'cli-isolated.json').write_text(json.dumps({'xdg_config_home':'fresh empty temporary directory; HOME unchanged','same_failures':True,'results':results},indent=2)+'\n')
