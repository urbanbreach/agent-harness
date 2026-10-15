import { useTheme } from '../theme';
import { Button, Icon } from '../ui';
import { t } from '../i18n';
import { useState } from 'react';

export function Welcome({
  selectedProject,
  onOpenProject,
  onProjectless,
  onProjectSelector,
}: {
  selectedProject?: { name: string; projectless?: boolean } | null;
  onOpenProject(): void;
  onProjectless(): void;
  onProjectSelector?(): void;
}) {
  const { colors, sp } = useTheme();
  const noProject = selectedProject == null;
  const [selectorWidth, setSelectorWidth] = useState(0);
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        flexGrow: 1,
        minHeight: 0,
        alignItems: 'center',
        justifyContent: 'center',
        paddingLeft: 32,
        paddingRight: 32,
        paddingBottom: noProject ? 46 : 52,
      }}
    >
      <Icon name="sparkle" size={noProject ? 24 : 20} color={colors.accent} />
      {noProject ? (
        <>
          <text
            style={{
              marginTop: 16,
              color: colors.text,
              fontSize: sp(20),
              lineHeight: sp(32),
              fontWeight: 500,
            }}
          >
            {t('welcome.title')}
          </text>
          <text
            style={{
              marginTop: 8,
              maxWidth: 380,
              color: colors.text_tertiary,
              fontSize: sp(12.5),
              lineHeight: sp(19),
              textAlign: 'center',
            }}
          >
            {t('welcome.description')}
          </text>
          <div
            style={{
              marginTop: 20,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              gap: 8,
            }}
          >
            <Button
              onClick={onOpenProject}
              style={{
                height: 32,
                paddingLeft: 14,
                paddingRight: 14,
                borderRadius: 999,
                borderWidth: 1,
                borderColor: 'transparent',
                backgroundColor: colors.inverse,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                cursor: 'default',
                hover: { opacity: 0.9 },
                active: { opacity: 0.8 },
              }}
            >
              <text style={{ color: colors.on_inverse, fontSize: sp(12.5), fontWeight: 600 }}>
                {t('welcome.openFolder')}
              </text>
            </Button>
            <Button
              onClick={onProjectless}
              style={{
                height: 30,
                paddingLeft: 12,
                paddingRight: 12,
                borderRadius: 999,
                borderWidth: 1,
                borderColor: 'transparent',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 6,
                cursor: 'default',
                hover: { backgroundColor: colors.overlay },
                active: { backgroundColor: colors.overlay_strong },
              }}
            >
              <Icon name="x" size={11} color={colors.text_tertiary} />
              <text style={{ color: colors.text_secondary, fontSize: sp(12.5) }}>
                {t('welcome.projectless')}
              </text>
            </Button>
          </div>
        </>
      ) : (
        <div
          style={{
            marginTop: 14,
            display: 'flex',
            alignItems: 'baseline',
            justifyContent: 'center',
          }}
        >
          <text
            style={{ color: colors.text, fontSize: sp(20), lineHeight: sp(28), fontWeight: 500 }}
          >
            {t(selectedProject.projectless ? 'welcome.greeting' : 'welcome.greetingIn')}
          </text>
          {!selectedProject.projectless && (
            <>
              <Button
                onLayout={(event) => setSelectorWidth(event.width ?? 0)}
                onClick={onProjectSelector}
                style={{ position: 'relative', cursor: 'default' }}
              >
                <text style={{ color: colors.text, fontSize: sp(20), fontWeight: 500 }}>
                  {selectedProject.name}
                </text>
                {selectorWidth > 0 && (
                  <svg
                    source={`<svg xmlns="http://www.w3.org/2000/svg" width="${selectorWidth}" height="1"><path d="M0 .5H${selectorWidth}" stroke="black" stroke-dasharray="1 2"/></svg>`}
                    style={{
                      position: 'absolute',
                      bottom: 0.5,
                      width: selectorWidth,
                      height: 1,
                      color: colors.text_tertiary,
                    }}
                  />
                )}
              </Button>
              <text style={{ color: colors.text, fontSize: sp(20), fontWeight: 500 }}>
                {t('welcome.question')}
              </text>
            </>
          )}
        </div>
      )}
    </div>
  );
}
