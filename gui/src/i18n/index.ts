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
  'clipboard.expression': 'Copy Expression',
  'clipboard.selection': 'Copy Selection',
} as const;

export type TranslationKey = keyof typeof english;
export type TranslationVariables = Readonly<Record<string, string | number>>;
export const language = 'en';

/** Both placeholder syntaxes are authored in the shared English catalog. */
export function t(key: TranslationKey, vars: TranslationVariables = {}): string {
  const text = english[key];
  return text.replace(/%?\{([^}]+)\}/g, (placeholder, name: string) => {
    const value = vars[name] ?? (name === 'AppName' ? 'Harness' : undefined);
    return value === undefined ? placeholder : String(value);
  });
}
