from pathlib import Path
import json,hashlib
root=Path('.omo/evidence/tui-rewrite/tool-layout');results=[]
a=json.loads((root/'selection-before/cells.json').read_text());b=json.loads((root/'selection-candidate/cells.json').read_text())
for name in a:
 differences=[k for k in a[name] if a[name][k]!=b[name].get(k)]
 p=root/'selection-before'/(name+'.png');q=root/'selection-candidate'/(name+'.png')
 assert p.read_bytes()==q.read_bytes(),name
 assert set(differences)<= {'renderCount'},(name,differences)
 results.append({'name':name,'pixels_cells_modes_equal':True,'observer_differences':{k:{'before':a[name][k],'candidate':b[name][k]} for k in differences},'candidate_png_sha256':hashlib.sha256(q.read_bytes()).hexdigest()})
for side in ['selection-before','selection-candidate']:
 report=json.loads((root/side/'report.json').read_text())
 assert report['highlighted'] and report['exit']['code']==0 and report['exit']['signal'] is None
 assert report['termios_restored'] and report['protocol_restored'] and report['temporary_root_removed']
 c=report['cleanup'];assert c['childExited'] and c['stdinClosed'] and not c['processGroupAlive'] and not c['terminatedByCleanup'] and not c['temporarySockets']
 c=report['browser_cleanup'];assert c['pageClosed'] and c['contextClosed'] and c['profileRemoved'] and not c['browserConnectedAfterClose'] and not c['boundPorts']
Path('/tmp/tui-tool-layout/selection-comparison.json').write_text(json.dumps({'comparisons':results,'both_restoration_and_cleanup_pass':True},indent=2)+'\n')
print('Both live selection PNGs/cells match; both fixtures restore terminal and release resources.')
