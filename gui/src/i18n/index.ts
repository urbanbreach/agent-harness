import { useSyncExternalStore } from 'react';

export const english = {
  'appearance.system': 'System',
  'appearance.light': 'Light',
  'appearance.dark': 'Dark',
  'appearance.uiSize': 'UI font size',
  'appearance.uiSizeHelp': 'Text size across the interface and messages',
  'appearance.codeSize': 'Code font size',
  'appearance.codeSizeHelp': 'Text size in the file editor, diffs, code blocks, and tool output',
  'action.about': 'About {AppName}',
  'action.checkUpdates': 'Check for Updates…',
  'action.settings': 'Settings…',
  'action.quit': 'Quit {AppName}',
  'action.file': 'File',
  'action.edit': 'Edit',
  'action.view': 'View',
  'action.window': 'Window',
  'action.newTask': 'New Task',
  'action.newProject': 'New Project…',
  'action.undo': 'Undo',
  'action.redo': 'Redo',
  'action.cut': 'Cut',
  'action.copy': 'Copy',
  'action.paste': 'Paste',
  'action.selectAll': 'Select All',
  'action.palette': 'Command Palette…',
  'action.sidebar': 'Toggle Sidebar',
  'action.rightPanel': 'Toggle Right Panel',
  'action.focusComposer': 'Focus Composer',
  'action.modelPicker': 'Toggle Model Picker',
  'action.usagePanel': 'Toggle Usage Panel',
  'action.fps': 'Toggle FPS Counter',
  'action.closeWindow': 'Close Window',
  'action.renameTask': 'Rename task',
  'action.exportMarkdown': 'Export Markdown…',
  'action.duplicateTask': 'Duplicate task',
  'action.collapseSidebarGroups': 'Collapse all sidebar groups',
  'input.find': 'Find in Conversation',
  'action.showReasoning': 'Toggle Reasoning',
  'action.showMessageTimes': 'Toggle Message Times',
  'frame.minimize': 'Minimize',
  'frame.maximize': 'Maximize',
  'frame.restore': 'Restore',
  'task.untitled': 'New task',
  'welcome.title': 'Open a project to begin',
  'welcome.description':
    '{AppName} runs coding agents in folders you choose. Your code, sessions, and history stay on this computer',
  'welcome.openFolder': 'Open project folder…',
  'welcome.projectless': "Don't work in a project",
  'welcome.withoutProject': 'without a project',
  'welcome.fallbackProject': 'your project',
  'welcome.greeting': 'What should we build?',
  'welcome.greetingIn': 'What should we build in\u00a0',
  'welcome.question': '?',
  'notice.dismiss': 'Dismiss notification',
  'notice.saveFailed': 'Could not save local state: %{error}',
  'task.completed': 'Turn completed',
  'task.stopped': 'Stopped',
  'task.background': 'Waiting for background tasks',
  'navigation.back': 'Back',
  'navigation.forward': 'Forward',
  'navigation.search': 'Search',
  'navigation.today': 'Today',
  'navigation.options': 'Sidebar options',
  'navigation.newProject': 'New project',
  'navigation.settings': 'Settings',
  'navigation.usage': 'Usage',
  'clipboard.code': 'Copy Code',
  'clipboard.copyDone': 'Copied',
  'transcript.changed_file': 'Changed %{count} file',
  'transcript.changed_files': 'Changed %{count} files',
  'transcript.review_changes': 'Review',
  'transcript.show_more_files': 'Show %{count} more files',
  'transcript.show_fewer_files': 'Show fewer files',
  'transcript.showing_first_files': 'Showing first %{count} of %{total}',
  'clipboard.expression': 'Copy Expression',
  'clipboard.selection': 'Copy Selection',
} as const;

export type TranslationKey = keyof typeof english;
export type TranslationVariables = Readonly<Record<string, string | number>>;
export type Language = 'system' | 'en' | 'zh-CN' | 'ja';
export let language: Language = 'system';
const listeners = new Set<() => void>();
export function setLanguage(value: Language) {
  if (language === value) return;
  language = value;
  listeners.forEach((listener) => listener());
}
export function useLanguage(): Language {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    () => language,
  );
}

/** Both placeholder syntaxes are authored in the shared English catalog. */
export function t(key: TranslationKey, vars: TranslationVariables = {}): string {
  const text = english[key];
  return text.replace(/%?\{([^}]+)\}/g, (placeholder, name: string) => {
    const value = vars[name] ?? (name === 'AppName' ? 'Harness' : undefined);
    return value === undefined ? placeholder : String(value);
  });
}
