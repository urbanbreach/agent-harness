from pathlib import Path
import json,hashlib,sys
base=Path(sys.argv[1]); output=Path(sys.argv[2]);rows=[]
for p in sorted((base/'before').glob('*.png')):
    q=base/'candidate'/p.name
    a=json.loads(p.with_suffix('.screen.json').read_text());b=json.loads(q.with_suffix('.screen.json').read_text())
    differences={k:{'before':a[k],'candidate':b.get(k)} for k in a if a[k]!=b.get(k)}
    equal=p.read_bytes()==q.read_bytes()
    assert equal,p.name
    assert set(differences)<= {'renderCount','parsedCount'},(p.name,list(differences))
    rows.append({'name':p.name,'pixels_equal':equal,'png_sha256':hashlib.sha256(q.read_bytes()).hexdigest(),'observer_differences':differences})
assert len(rows)==27
reports=[]
for side in ['before','candidate']:
    d=json.loads((base/side/'report.json').read_text())
    assert d['exit']['code']==0 and d['termios_restored'] and d['protocol_restored'] and d['temporary_root_removed']
    assert d['cleanup']['childExited'] and not d['cleanup']['processGroupAlive'] and not d['cleanup']['terminatedByCleanup'] and not d['cleanup']['temporarySockets']
    screen=lambda name: json.loads((base/side/f'{name}.screen.json').read_text())
    prompt=screen('01-prompt'); draft=screen('11-redo-selection'); cleared=screen('12b-cleared')
    assert cleared['cursor']==prompt['cursor']
    cursor=cleared['cursor']
    ghost=[c for c in cleared['cells'] if c['row']==cursor['y'] and cursor['x']<=c['column']<cursor['x']+len('undo branch')]
    prior=[c for c in draft['cells'] if c['row']==cursor['y'] and cursor['x']<=c['column']<cursor['x']+len('undo branch')]
    assert ''.join(c['chars'] for c in ghost)=='undo branch'
    assert all(a['italic'] and not b['italic'] and a['fgColor']!=b['fgColor'] for a,b in zip(ghost,prior))
    assert all('undo branch' not in screen(name)['text'] for name in ['13-multiline-paste','14-expanded-paste'])
    reports.append(d)
    b=d['browser_cleanup']
    assert b['pageClosed'] and b['contextClosed'] and not b['browserConnectedAfterClose'] and b['profileRemoved'] and not b['boundPorts']
# Each run has a fresh temporary workspace; all keyboard/event fields otherwise match.
for report in reports:
    data=report['inputs'][0]['event']['payload']['data']
    assert Path(data['workspace_root']) == Path(report['browser_cleanup']['profilePath']).parent/'workspace'
    data['workspace_root']='<isolated fixture workspace>'
assert reports[0]['inputs']==reports[1]['inputs']
a,b=[report['emulator'] for report in reports]
assert all(a[k]==b.get(k) for k in a if k!='terminal')
assert {k for k in a['terminal'] if a['terminal'][k]!=b['terminal'].get(k)} <= {'renderCount','parsedCount'}
output.write_text(json.dumps({'comparisons':rows,'both_runs_restore_and_cleanup':True,'clear_then_history_ghost_and_unprefixed_paste':True},indent=2)+'\n')
print('27 exact paired PNG/cell/cursor captures; both runs restored and cleaned up')
