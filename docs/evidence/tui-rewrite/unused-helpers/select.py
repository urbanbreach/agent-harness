from pathlib import Path
import hashlib,json,re,subprocess
root=Path('crates/harness-tui/src');out=Path(__file__).resolve().parent
selected={
 'app.rs':['provider_disconnected'],
 'app/activity.rs':['merge_orchestration_task_completion_metadata'],
 'app/lifecycle.rs':['startup_card_view_model'],
 'app/model_switcher.rs':['runtime_context_profile'],
 'app/palette_controller.rs':['is_suggested'],
 'app/permissions.rs':['mark_resolved'],
 'app/permissions/question.rs':['question_prompt_is_single_select'],
 'app/session_history.rs':['session_history_time_label','format_twelve_hour_time','session_history_date_parts','epoch_millis_date_parts','epoch_millis_time_parts','current_utc_date','civil_from_days','days_from_civil','weekday_name','month_name','session_history_visual_row_count'],
 'app/session_navigation.rs':['slash_command_column_width','palette_command_available'],
 'app/session_slash.rs':['slash_command_display_width'],
 'app/transcript_state.rs':['active_turn_tool_motion_demand','show_mode_banner'],
 'layout/overlays.rs':['command_palette_visible_rows'],
 'startup_logo.rs':['row_spans','padded_row'],
 'ui_chrome.rs':['command_palette_section','fork_selector_selection_bg','fork_selector_selection_fg','preferred_binding'],
 'ui_permission_dock.rs':['question_prompt_secondary'],
 'ui_lsp.rs':['path_root_from_args'],
 'ui_overlays.rs':['render_command_palette_surface'],
 'ui_overlays/permission_modal.rs':['permission_modal_metadata_line','permission_modal_icon','permission_modal_guidance','permission_modal_summary_line','permission_modal_draft_line'],
 'ui_overlays/release_notes.rs':['push_wrapped_bullet'],
 'ui_transcript_types.rs':['assistant_reasoning','assistant_bodies','assistant_error'],
 'ui_transcript_render.rs':['retry_attempt'],
 'ui_transcript_scrollbar.rs':['transcript_scroll_offset'],
 'ui_transcript_style.rs':['transcript_nested_rail_color'],
 'view_model.rs':['startup_card_view_model'],
 'rewind_view.rs':['new_cancel_offer'],
}
rows=[];chunks=[]
for name,functions in selected.items():
 path=root/name;lines=path.read_text().splitlines(keepends=True)
 for function in functions:
  starts=[i for i,line in enumerate(lines) if re.search(rf'\bfn {function}\b',line)]
  assert len(starts)==1,(name,function,starts)
  start=starts[0];indent=re.match(r'\s*',lines[start])[0]
  end=next(i+1 for i in range(start+1,len(lines)) if lines[i]==indent+'}\n')
  block=''.join(lines[start:end]);refs=subprocess.check_output(['rg','-n',rf'\b{function}\b','crates','scripts','--glob','*.rs']).decode().splitlines()
  rows.append({'path':str(path),'name':function,'start':start+1,'end':end,'sha256':hashlib.sha256(block.encode()).hexdigest(),'references':refs})
  chunks.append(f'{path}:{start+1}\n{block}')
(out/'selected.json').write_text(json.dumps({'base':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'functions':rows},indent=2)+'\n')
(out/'selected-source.txt').write_text('\n'.join(chunks))
print(len(rows),'functions',sum(row['end']-row['start']+1 for row in rows),'lines',len(selected),'files')
