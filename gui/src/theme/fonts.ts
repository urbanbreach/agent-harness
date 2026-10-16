import { readFile } from 'node:fs/promises';

export const UI_FONT_FAMILY = '.SystemUIFont';
export const CODE_FONT_FAMILY = 'JetBrains Mono';
export const FONT_SIZE_CHOICES = [11, 12, 13, 14, 15, 16, 18, 20] as const;
export const DEFAULT_UI_FONT_SIZE = 14;
export const DEFAULT_CODE_FONT_SIZE = 13;

export function clampFontSize(value: number, fallback: number): number {
  return Number.isFinite(value) ? Math.min(24, Math.max(9, value)) : fallback;
}

export interface FontRegistrar {
  registerFont(data: Buffer): void | Promise<void>;
}

const registrations = new WeakMap<FontRegistrar, Promise<void>>();

/** Register before mounting any code text: the backend caches font resolution. */
export function registerFonts(renderer: FontRegistrar): Promise<void> {
  const registered = registrations.get(renderer);
  if (registered) return registered;
  const registration = (async () => {
    const faces = ['Regular', 'Bold', 'Italic', 'BoldItalic'];
    const data = await Promise.all(
      faces.map((face) =>
        readFile(new URL(`../../assets/fonts/JetBrainsMono-${face}.ttf`, import.meta.url)),
      ),
    );
    for (const bytes of data) await renderer.registerFont(bytes);
  })();
  registrations.set(renderer, registration);
  return registration;
}
