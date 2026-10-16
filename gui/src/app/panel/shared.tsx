import type { ReactNode } from 'react';
import { useTheme } from '../../theme';
import { Icon, row } from '../../ui';
export function EmptyMessage({
  title,
  description,
  icon,
  children,
  bottomPadding = 32,
}: {
  title: string;
  description?: string;
  icon?: string;
  children?: ReactNode;
  bottomPadding?: number;
}) {
  const { colors, sp } = useTheme();
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        alignItems: 'center',
        justifyContent: 'center',
        paddingLeft: 48,
        paddingRight: 48,
        paddingBottom: bottomPadding,
      }}
    >
      {icon && (
        <div style={{ marginBottom: 14 }}>
          <Icon name={icon} size={22} />
        </div>
      )}
      <text style={{ color: colors.text, fontSize: sp(13), fontWeight: 500 }}>{title}</text>
      {description && (
        <text
          style={{
            color: colors.text_tertiary,
            fontSize: sp(12.5),
            lineHeight: sp(17),
            textAlign: 'center',
            maxWidth: 340,
            marginTop: 6,
          }}
        >
          {description}
        </text>
      )}
      {children}
    </div>
  );
}
export function FileIcon({ path, size = 14 }: { path: string; size?: number }) {
  const extension = path.split('.').pop()?.toLowerCase();
  const names: Record<string, string> = {
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    md: 'markdown',
    rs: 'rust',
    json: 'json',
    css: 'css',
    html: 'html',
    py: 'python',
    sh: 'shell',
    yml: 'yaml',
    yaml: 'yaml',
  };
  const name = names[extension ?? ''];
  return name ? (
    <svg
      src={new URL(`../../../assets/file-icons/${name}.svg`, import.meta.url).pathname}
      style={{ width: size, height: size, flexShrink: 0 }}
    />
  ) : (
    <Icon name="file" size={size} />
  );
}
export function Toolbar({ children }: { children: ReactNode }) {
  const { colors } = useTheme();
  return (
    <div
      style={{
        ...row,
        height: 42,
        flexShrink: 0,
        paddingLeft: 16,
        paddingRight: 16,
        gap: 8,
        borderBottomWidth: 1,
        borderColor: colors.border,
      }}
    >
      {children}
    </div>
  );
}
