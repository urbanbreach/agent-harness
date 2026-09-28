from pathlib import Path
from collections import Counter
import json,re
out=Path(__file__).resolve().parent
stacks=(out/'allocation-stacks.log').read_text().split('#0 ')[1:]
nearest=Counter();inclusive=Counter()
for stack in stacks:
 locations=re.findall(r' at (crates/harness-tui/src/[^\s]+)',stack)
 nearest[locations[0] if locations else '(no production frame within 30 frames)']+=1
 inclusive.update(set(locations))
summary={'purpose':'Malloc stack sampling every 200 hits from separate symbolized release build; includes setup/warmup, excludes calloc/realloc. No latency acceptance claim.','stack_count':len(stacks),'first_production_location':nearest.most_common(),'inclusive_production_location':inclusive.most_common()}
(out/'allocation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
