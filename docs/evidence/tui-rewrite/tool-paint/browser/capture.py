from pathlib import Path
import hashlib,json,shutil,subprocess
base=Path('/tmp/tui-tool-paint');output=Path('.omo/evidence/tui-rewrite/tool-paint')
names=['patch-apply-patch','ranged-read-fs-read','execute-shell-run','sent-agent-message','unknown-fixture-inspect','mcp-use-mcp-linear-save-issue']
for side in ['before','candidate']:
 selected=base/(side+'-selected');selected.mkdir(exist_ok=True)
 for name in names:
  stem=f'body-{name}-success-open-120x40-motion-0ms.ansi'
  shutil.copyfile(base/(side+'-body')/stem,selected/stem)
 shutil.copyfile(base/(side+'-body')/'producer.json',selected/'producer.json')
 command=['node','scripts/qa/render-recorded-frames.mjs',str(selected),str(output/('recorded-'+side))]
 with (base/('browser-'+side+'.log')).open('w') as log:subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(side,'captured',flush=True)
comparisons=[]
for p in sorted((output/'recorded-before').glob('*.png')):
 q=output/'recorded-candidate'/p.name
 assert p.read_bytes()==q.read_bytes(),p.name
 a=json.loads(p.with_suffix('.screen.json').read_text());b=json.loads(q.with_suffix('.screen.json').read_text())
 differences={k:{'before':a[k],'candidate':b.get(k)} for k in a if a[k]!=b.get(k)}
 assert set(differences)<={'renderCount'},(p.name,list(differences))
 comparisons.append({'name':p.name,'pixels_cells_modes_equal':True,'candidate_png_sha256':hashlib.sha256(q.read_bytes()).hexdigest(),'observer_differences':differences})
(base/'browser-comparison.json').write_text(json.dumps(comparisons,indent=2)+'\n')
print('Six paired tool detail PNGs/cells/modes match',flush=True)
