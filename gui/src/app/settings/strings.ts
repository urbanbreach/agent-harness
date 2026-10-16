import { useLanguage } from '../../i18n';

const translations: Record<string, [string, string]> = {
  General: ['常规', '一般'],
  Appearance: ['外观', '外観'],
  Providers: ['提供商', 'プロバイダー'],
  Runtime: ['运行时', 'ランタイム'],
  'Tools & MCP': ['工具与 MCP', 'ツールと MCP'],
  Memory: ['记忆', 'メモリ'],
  Skills: ['技能', 'スキル'],
  Usage: ['用量', '使用状況'],
  Diagnostics: ['诊断', '診断'],
  Back: ['返回', '戻る'],
  'Search Settings': ['搜索设置', '設定を検索'],
  'Local by default': ['默认保存在本地', 'ローカル保存が標準'],
  'Projects, conversations, and settings are stored on this computer': [
    '项目、对话和设置保存在此计算机上',
    'プロジェクト、会話、設定はこのコンピューターに保存されます',
  ],
  'Render math expressions': ['渲染数学表达式', '数式を表示'],
  'Show formatted math in Markdown. Turn off to show LaTeX source.': [
    '在 Markdown 中显示格式化的数学表达式。关闭后显示 LaTeX 源代码。',
    'Markdown の数式を整形して表示します。オフにすると LaTeX ソースを表示します。',
  ],
  'Show reasoning': ['显示推理', '推論を表示'],
  'Show the agent’s reasoning in the conversation.': [
    '在对话中显示智能体的推理过程。',
    '会話にエージェントの推論を表示します。',
  ],
  'Show message times': ['显示消息时间', 'メッセージ時刻を表示'],
  'Display timestamps beside conversation messages.': [
    '在对话消息旁显示时间戳。',
    '会話のメッセージに時刻を表示します。',
  ],
  Theme: ['主题', 'テーマ'],
  Language: ['语言', '言語'],
  System: ['系统', 'システム'],
  Light: ['浅色', 'ライト'],
  Dark: ['深色', 'ダーク'],
  'Choose between system, light, or dark themes': [
    '选择系统、浅色或深色主题',
    'システム、ライト、ダークのテーマから選択',
  ],
  'Choose the language used throughout Harness': [
    '选择 Harness 使用的语言',
    'Harness 全体で使用する言語を選択',
  ],
  'UI font size': ['界面字号', 'UI フォントサイズ'],
  'Code font size': ['代码字号', 'コードのフォントサイズ'],
  'Text size across the interface and messages': [
    '界面和消息中的文字大小',
    'インターフェースとメッセージの文字サイズ',
  ],
  'Text size in the file editor, diffs, code blocks, and tool output': [
    '文件编辑器、差异、代码块和工具输出中的文字大小',
    'ファイルエディター、差分、コードブロック、ツール出力の文字サイズ',
  ],
};
export function useSettingsText() {
  const preference = useLanguage();
  const locale = preference === 'system' ? (process.env.LANG ?? 'en') : preference;
  return (text: string) =>
    translations[text]?.[locale.startsWith('zh') ? 0 : 1] &&
    (locale.startsWith('zh') || locale.startsWith('ja'))
      ? translations[text]![locale.startsWith('zh') ? 0 : 1]
      : text;
}
