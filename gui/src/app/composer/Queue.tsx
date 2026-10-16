import { useTheme } from '../../theme';
import { Button, Dropdown, Icon, IconButton, Tooltip } from '../../ui';
import type { Draft } from '../../store/store';
import { canSteerDraft } from './queue-store';
export interface FollowUp {
  id: string;
  draft: Draft;
}
export function Queue({
  items,
  onEdit,
  onRemove,
  onSteer,
}: {
  items: FollowUp[];
  onEdit: (item: FollowUp) => void;
  onRemove: (id: string) => void;
  onSteer: (item: FollowUp) => void;
}) {
  const { colors, sp } = useTheme();
  if (!items.length) return null;
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        borderTopLeftRadius: 12,
        borderTopRightRadius: 12,
        borderWidth: 1,
        borderBottomWidth: 0,
        borderColor: colors.border,
        backgroundColor: colors.composer,
        paddingTop: 4,
        paddingBottom: 4,
        paddingLeft: 14,
        paddingRight: 14,
        overflow: 'hidden',
      }}
    >
      {items.map((item) => (
        <div
          key={item.id}
          style={{
            height: 30,
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            gap: 9,
            paddingLeft: 12,
            paddingRight: 6,
            borderRadius: 6,
            hover: { backgroundColor: colors.overlay },
          }}
        >
          <Icon name="queue" size={12} />
          <Tooltip content="Edit in composer">
            <Button
              onClick={() => onEdit(item)}
              style={{ flexGrow: 1, flexShrink: 1, minWidth: 0, justifyContent: 'flex-start' }}
            >
              <text
                style={{
                  fontSize: sp(12.5),
                  color: colors.text,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {item.draft.text ||
                  item.draft.attachments?.map((attachment) => attachment.name).join(', ')}
              </text>
            </Button>
          </Tooltip>
          <Tooltip content="Steer current turn">
            <Button
              disabled={!canSteerDraft(item.draft)}
              onClick={() => onSteer(item)}
              style={{ height: 24, paddingLeft: 7, paddingRight: 7, gap: 5, borderRadius: 6 }}
            >
              <Icon name="corner-down-right" size={11} />
              <text style={{ fontSize: sp(12.5) }}>Steer</text>
            </Button>
          </Tooltip>
          <Tooltip content="Remove from queue">
            <IconButton
              name="trash"
              label="Remove from queue"
              size={12}
              style={{ width: 24, height: 24 }}
              onClick={() => onRemove(item.id)}
            />
          </Tooltip>
          <Dropdown
            icon="ellipsis"
            label=""
            aria-label="Follow-up actions"
            chevron={false}
            style={{ width: 24, height: 24, paddingLeft: 4, paddingRight: 4 }}
            items={[
              {
                id: 'edit',
                label: 'Edit in composer',
                icon: 'pencil',
                onSelect: () => onEdit(item),
              },
              {
                id: 'remove',
                label: 'Remove from queue',
                icon: 'trash',
                onSelect: () => onRemove(item.id),
              },
            ]}
          />
        </div>
      ))}
    </div>
  );
}
