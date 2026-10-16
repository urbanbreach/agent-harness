import { useState } from 'react';
import { useTheme } from '../../theme';
import { Button, ContextMenu, Dialog, Icon, IconButton, Tooltip } from '../../ui';
import { revealPath } from '../../workspace/os';
import type { DraftAttachment } from '../../store/store';
export function AttachmentTiles({
  attachments,
  onRemove,
}: {
  attachments: DraftAttachment[];
  onRemove: (index: number) => void;
}) {
  const { colors, sp } = useTheme();
  const [preview, setPreview] = useState<DraftAttachment>();
  return (
    <>
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          flexWrap: 'wrap',
          gap: 8,
          paddingLeft: 14,
          paddingRight: 14,
          paddingTop: 2,
          paddingBottom: 8,
        }}
      >
        {attachments.map((attachment, index) => (
          <ContextMenu
            key={`${attachment.path ?? attachment.name}-${index}`}
            items={[
              {
                id: 'reveal',
                label: 'Show in File Manager',
                icon: 'folder',
                disabled: !attachment.path,
                onSelect: () => {
                  if (attachment.path) void revealPath(attachment.path);
                },
              },
            ]}
            style={{ position: 'relative', width: 64, height: 64 }}
          >
            <Tooltip content={`@${attachment.path ?? attachment.name}`}>
              <Button
                aria-label={attachment.name ?? 'Attachment'}
                onClick={() => {
                  if (attachment.mime.startsWith('image/')) setPreview(attachment);
                }}
                style={{
                  width: 64,
                  height: 64,
                  borderRadius: 8,
                  borderColor: colors.border,
                  backgroundColor: colors.inset,
                  overflow: 'hidden',
                  flexDirection: 'column',
                  gap: 5,
                  paddingLeft: 5,
                  paddingRight: 5,
                }}
              >
                {attachment.dataBase64 ? (
                  <img
                    src={`data:${attachment.mime};base64,${attachment.dataBase64}`}
                    objectFit="cover"
                    style={{ width: 64, height: 64 }}
                  />
                ) : (
                  <>
                    <Icon name={attachment.directory ? 'folder' : 'file'} size={16} />
                    <text
                      style={{
                        maxWidth: 54,
                        fontSize: sp(12.5),
                        color: colors.text_tertiary,
                        whiteSpace: 'nowrap',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                      }}
                    >
                      {attachment.name}
                    </text>
                  </>
                )}
              </Button>
            </Tooltip>
            <IconButton
              name="x"
              label={`Remove ${attachment.name}`}
              size={9}
              onClick={() => onRemove(index)}
              style={{
                position: 'absolute',
                top: 3,
                right: 3,
                width: 16,
                height: 16,
                borderRadius: 5,
                backgroundColor: colors.canvas,
                opacity: 0.8,
                hover: { opacity: 0.95 },
              }}
            />
          </ContextMenu>
        ))}
      </div>
      <Dialog
        open={!!preview}
        onClose={() => setPreview(undefined)}
        title="Image preview"
        scrimColor="rgba(0,0,0,.82)"
        style={{
          maxWidth: '100%',
          height: '100%',
          backgroundColor: 'transparent',
          padding: 36,
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <IconButton
          name="x"
          label="Close image preview"
          onClick={() => setPreview(undefined)}
          style={{
            position: 'absolute',
            top: 14,
            right: 14,
            width: 32,
            height: 32,
            borderRadius: 32,
            backgroundColor: '#0000007a',
          }}
          color="#ffffff"
        />
        {preview?.dataBase64 ? (
          <img
            src={`data:${preview.mime};base64,${preview.dataBase64}`}
            objectFit="contain"
            style={{ flexGrow: 1, width: '100%', minHeight: 0 }}
          />
        ) : (
          <text style={{ color: '#ffffffc7' }}>Image unavailable</text>
        )}
        <text
          style={{
            marginTop: 12,
            paddingLeft: 11,
            paddingRight: 11,
            paddingTop: 5,
            paddingBottom: 5,
            borderRadius: 30,
            maxWidth: 560,
            backgroundColor: '#0000007a',
            color: '#ffffffe6',
            fontSize: sp(12.5),
          }}
        >
          {preview?.name}
        </text>
      </Dialog>
    </>
  );
}
