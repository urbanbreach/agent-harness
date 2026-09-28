from pathlib import Path
import collections,hashlib,json,re
out=Path(__file__).resolve().parent
rows=json.loads((out/'selected.json').read_text())['functions'];groups=collections.defaultdict(list)
for row in rows:groups[Path(row['path'])].append(row)
changes={}
for path,rows in groups.items():
 lines=path.read_text().splitlines(keepends=True)
 for row in sorted(rows,key=lambda r:r['start'],reverse=True):
  a,b=row['start']-1,row['end'];block=''.join(lines[a:b])
  assert hashlib.sha256(block.encode()).hexdigest()==row['sha256'],row
  if b<len(lines) and not lines[b].strip():b+=1
  del lines[a:b]
 changes[path]=''.join(lines)
root=Path('crates/harness-tui/src')
def change(name,old,new=''):
 p=root/name;s=changes.get(p,p.read_text());assert old in s,(name,old);changes[p]=s.replace(old,new)
change('app.rs','/// Truncation limit for tool output display in the TUI (chars)\nconst TOOL_OUTPUT_DISPLAY_MAX_CHARS: usize = 100;\n\n')
change('ui_transcript_types.rs','pub(super) const TRANSCRIPT_REASONING_BODY_PREFIX: &str = TRANSCRIPT_ASSISTANT_BODY_PREFIX;\n')
change('ui_transcript_types.rs','pub(super) const TRANSCRIPT_REASONING_HEADER_PREFIX: &str = TRANSCRIPT_ASSISTANT_BODY_PREFIX;\n')
change('ui_transcript_mermaid.rs','const MIN_NODE_WIDTH: usize = 7;\nconst MAX_LABEL_WIDTH: usize = 28;\n')
change('view_model.rs','#[derive(Debug, Clone, PartialEq, Eq)]\npub(crate) struct StartupCardViewModel {\n    pub metadata: String,\n}\n\n')
change('app/permissions.rs','impl PermissionEntry {\n}\n\n')
change('rewind_view.rs','impl RewindState {\n}\n\n')
imports={
 'app.rs':['merge_orchestration_task_completion_metadata','TaskCompletionMetadata'],
 'app/session_projection.rs':['merge_orchestration_task_completion_metadata','TOOL_OUTPUT_DISPLAY_MAX_CHARS'],
 'app/session_navigation.rs':['slash_command_display_width'],
 'app/palette_controller.rs':['SuggestedRule'],
 'ui_overlays.rs':['permission_modal_draft_line','permission_modal_guidance','permission_modal_icon','permission_modal_metadata_line','permission_modal_summary_line'],
 'ui_permission_dock.rs':['permission_modal_draft_line','permission_modal_guidance','permission_modal_metadata_line','permission_modal_summary_line'],
 'ui_chrome.rs':['question_prompt_secondary'],
 'ui_transcript.rs':['transcript_scroll_offset','transcript_nested_rail_color'],
}
for name,items in imports.items():
 for item in items:
  p=root/name;source=changes.get(p,p.read_text())
  source,count=re.subn(r'\b'+item+r',\s*','',source,count=1)
  if not count:source,count=re.subn(r',\s*\b'+item+r'\b','',source,count=1)
  assert count==1,(name,item)
  changes[p]=source
change('ui_transcript_render.rs','use harness_core::event::ProviderRequestRetryMetadata;\n')
for path,source in changes.items():path.write_text(source)
print(len(changes),'source files changed')
