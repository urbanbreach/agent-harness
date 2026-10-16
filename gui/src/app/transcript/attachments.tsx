import { useEffect, useState } from 'react';
import { useTheme } from '../../theme';
import { getAppStore } from '../../store';
import { Button, Dialog, Icon, Tooltip } from '../../ui';
import type { AttachmentMetadata } from '../../model/harness-events';
import type { Attachment } from '../../backend/protocol';

export async function loadPromptAttachments(
  attachments: AttachmentMetadata[],
): Promise<Attachment[]> {
  return Promise.all(
    attachments.map(async (attachment) => {
      const blob = await getAppStore().backend.call('blobs.get', {
        digest: attachment.content_ref,
      });
      if (!blob)
        throw new Error(
          'The original attachment is unavailable. Copy this prompt to the composer and reattach the file.',
        );
      return { mime: blob.mime, dataBase64: blob.dataBase64 };
    }),
  );
}

export function AttachmentTile({ attachment }: { attachment: AttachmentMetadata }) {
  const { colors, sp } = useTheme();
  const [source, setSource] = useState<string>();
  const [preview, setPreview] = useState(false);
  useEffect(() => {
    let active = true;
    if (attachment.mime.startsWith('image/')) {
      void getAppStore()
        .backend.call('blobs.get', { digest: attachment.content_ref })
        .then((blob) => {
          if (active && blob) setSource(`data:${blob.mime};base64,${blob.dataBase64}`);
        })
        .catch(() => {
          if (active) setSource(undefined);
        });
    }
    return () => {
      active = false;
    };
  }, [attachment.content_ref, attachment.mime]);
  return (
    <>
      <Tooltip content={attachment.detail ?? attachment.id}>
        <Button
          disabled={!source}
          onClick={() => setPreview(true)}
          style={{
            width: 96,
            height: 80,
            borderRadius: 9,
            borderColor: colors.border,
            backgroundColor: colors.inset,
            flexDirection: 'column',
            gap: 7,
            overflow: 'hidden',
            padding: 0,
          }}
        >
          {source ? (
            <img src={source} objectFit="cover" style={{ width: 96, height: 80 }} />
          ) : (
            <>
              <Icon name={attachment.mime.startsWith('image/') ? 'image' : 'file'} size={18} />
              <text
                style={{
                  color: colors.text_secondary,
                  fontSize: sp(12.5),
                  maxWidth: 82,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {attachment.detail ?? attachment.id}
              </text>
            </>
          )}
        </Button>
      </Tooltip>
      {preview && source && (
        <Dialog open title={attachment.detail ?? 'Image preview'} onClose={() => setPreview(false)}>
          <img
            src={source}
            objectFit="contain"
            style={{
              maxWidth: 900,
              maxHeight: 650,
              width: attachment.dimensions?.width ?? 300,
              height: attachment.dimensions?.height ?? 200,
            }}
          />
        </Dialog>
      )}
    </>
  );
}
