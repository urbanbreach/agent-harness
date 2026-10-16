import { useEffect, useState } from 'react';
import { useTheme } from '../../theme';
import { Button, Icon, Tooltip, ellipsis, row } from '../../ui/controls';
import { t } from '../../i18n';
import { registerAction, runAction } from '../actions';
import { openPanelReviewTurn } from '../panel/index';
import { useTurnCheckpoint } from './checkpoints';

export function CheckpointCard({ sessionId, turnId }: { sessionId: string; turnId: string }) {
  const checkpoint = useTurnCheckpoint(sessionId, turnId);
  const { colors, sp } = useTheme();
  const [expanded, setExpanded] = useState(false);
  const actionId = `review.turn:${sessionId}:${turnId}`;
  useEffect(() => {
    setExpanded(false);
    return registerAction({
      id: actionId,
      label: 'transcript.review_changes',
      icon: 'file-diff',
      enabled: () => checkpoint?.state === 'ready',
      run: () => openPanelReviewTurn(sessionId, turnId),
    });
  }, [actionId, checkpoint, sessionId, turnId]);
  if (checkpoint?.state !== 'ready' || !checkpoint.files.length) return null;
  const count = checkpoint.files.length;
  const files = checkpoint.files.slice(0, expanded ? 12 : 3);
  const textStyle = { fontSize: sp(12.5), lineHeight: sp(20) };
  return (
    <div
      style={{
        minWidth: 0,
        width: '100%',
        marginBottom: 3,
        borderRadius: 12,
        borderWidth: 1,
        borderColor: colors.border_strong,
        backgroundColor: colors.overlay,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          ...row,
          minHeight: 58,
          paddingLeft: 12,
          paddingRight: 12,
          paddingTop: 9,
          paddingBottom: 9,
          gap: 10,
        }}
      >
        <div
          style={{
            ...row,
            justifyContent: 'center',
            width: 36,
            height: 36,
            flexShrink: 0,
            borderRadius: 9,
            backgroundColor: colors.overlay_strong,
          }}
        >
          <Icon name="file-diff" size={16} color={colors.text_tertiary} />
        </div>
        <div style={{ minWidth: 0, flexGrow: 1, gap: 0, position: 'relative', top: 2 }}>
          <div style={{ ...textStyle, fontWeight: 500, color: colors.text }}>
            {t(count === 1 ? 'transcript.changed_file' : 'transcript.changed_files', { count })}
          </div>
          <div style={{ ...row, ...textStyle, gap: 6 }}>
            <div style={{ color: colors.success }}>{`+${checkpoint.additions}`}</div>
            <div style={{ color: colors.danger }}>{`-${checkpoint.deletions}`}</div>
          </div>
        </div>
        <Button
          aria-label={t('transcript.review_changes')}
          onClick={() => runAction(actionId)}
          style={{
            height: 28,
            paddingLeft: 10,
            paddingRight: 10,
            borderRadius: 7,
            borderColor: colors.border_strong,
            ...textStyle,
            fontWeight: 500,
            gap: 5,
            flexShrink: 0,
            hover: { backgroundColor: colors.overlay_strong, color: colors.text },
            active: { backgroundColor: colors.overlay },
          }}
        >
          <Icon name="file-diff" size={12} color={colors.text_tertiary} />
          <div>{t('transcript.review_changes')}</div>
        </Button>
      </div>
      <div style={{ borderTopWidth: 1, borderColor: colors.border }}>
        {files.map((file) => (
          <div
            key={file.path}
            style={{ ...row, ...textStyle, height: 31, paddingLeft: 12, paddingRight: 12, gap: 8 }}
          >
            <div style={{ minWidth: 0, flexGrow: 1, flexShrink: 1 }}>
              <Tooltip content={file.path}>
                <div style={{ ...ellipsis, color: colors.text_secondary }}>{file.path}</div>
              </Tooltip>
            </div>
            {file.additions !== undefined && (
              <div style={{ color: colors.success, flexShrink: 0 }}>{`+${file.additions}`}</div>
            )}
            {file.deletions !== undefined && (
              <div style={{ color: colors.danger, flexShrink: 0 }}>{`-${file.deletions}`}</div>
            )}
          </div>
        ))}
      </div>
      {count > 3 && (
        <Button
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
          style={{
            ...textStyle,
            justifyContent: 'flex-start',
            minWidth: 0,
            height: 34,
            paddingLeft: 12,
            paddingRight: 12,
            borderTopWidth: 1,
            borderColor: colors.border,
            gap: 6,
            fontWeight: 500,
            hover: { backgroundColor: colors.overlay_strong },
            active: { backgroundColor: colors.overlay },
          }}
        >
          <div>
            {expanded
              ? t('transcript.show_fewer_files')
              : t('transcript.show_more_files', { count: count - 3 })}
          </div>
          {expanded && count > 12 && (
            <div style={{ ...ellipsis, fontWeight: 400, color: colors.text_ghost }}>
              {t('transcript.showing_first_files', { count: 12, total: count })}
            </div>
          )}
          <Icon
            name={expanded ? 'chevron-up' : 'chevron-down'}
            size={11}
            color={colors.text_tertiary}
          />
        </Button>
      )}
    </div>
  );
}
