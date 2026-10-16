import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useGpuix } from '@gpuix/react';
import { memoryPersist, type Persist } from '../state/persist';
import { darkColors, lightColors, type ThemeColors } from './colors';
import {
  clampFontSize,
  DEFAULT_CODE_FONT_SIZE,
  DEFAULT_UI_FONT_SIZE,
  registerFonts,
  type FontRegistrar,
} from './fonts';

export * from './colors';
export * from './fonts';

export type ThemePreference = 'system' | 'light' | 'dark';
export interface ThemePlatform extends FontRegistrar {
  windowAppearance?(): string;
  setWindowAppearance?(appearance: ThemePreference): void;
  setRemSize?(size: number): void;
  reducedMotion?(): boolean;
}
interface Preferences {
  preference: ThemePreference;
  uiFontSize: number;
  codeFontSize: number;
}
export interface ThemeValue extends Preferences {
  colors: ThemeColors;
  sp(value: number): number;
  setPreference(value: ThemePreference): void;
  setUiFontSize(value: number): void;
  setCodeFontSize(value: number): void;
  reducedMotion: boolean;
}
const defaults: Preferences = {
  preference: 'system',
  uiFontSize: DEFAULT_UI_FONT_SIZE,
  codeFontSize: DEFAULT_CODE_FONT_SIZE,
};
const ThemeContext = createContext<ThemeValue | undefined>(undefined);
export const THEME_PERSIST_KEY = 'appearance';

/** Native appearance is deliberately isolated; an unresolved System theme is dark. */
export function resolveAppearance(preference: ThemePreference, system?: string): 'light' | 'dark' {
  if (preference !== 'system') return preference;
  return system === 'light' || system === 'vibrant-light' ? 'light' : 'dark';
}

export function ThemeProvider({
  children,
  persist = memoryPersist,
  platform,
  reducedMotion: motionOverride,
  onError = console.error,
}: {
  children: React.ReactNode;
  persist?: Persist;
  platform?: ThemePlatform;
  onError?: (error: unknown) => void;
  reducedMotion?: boolean;
}) {
  const { renderer } = useGpuix();
  const native = platform ?? (renderer as unknown as ThemePlatform | null);
  const [preferences, setPreferences] = useState<Preferences>(defaults);
  const [ready, setReady] = useState(false);
  const [failure, setFailure] = useState<unknown>();
  const [systemAppearance, setSystemAppearance] = useState<string>();
  const [reducedMotion, setReducedMotion] = useState(false);
  const errorHandler = useRef(onError);
  const lastPreferences = useRef('');
  errorHandler.current = onError;

  useEffect(() => {
    let active = true;
    setReady(false);
    const load = async () => {
      if (native) await registerFonts(native);
      const saved = await persist.read<Partial<Preferences>>(THEME_PERSIST_KEY).catch((error) => {
        if (active) errorHandler.current(error);
        return undefined;
      });
      if (!active) return;
      const restored: Preferences = {
        preference:
          saved?.preference === 'light' || saved?.preference === 'dark'
            ? saved.preference
            : 'system',
        uiFontSize: clampFontSize(saved?.uiFontSize ?? DEFAULT_UI_FONT_SIZE, DEFAULT_UI_FONT_SIZE),
        codeFontSize: clampFontSize(
          saved?.codeFontSize ?? DEFAULT_CODE_FONT_SIZE,
          DEFAULT_CODE_FONT_SIZE,
        ),
      };
      lastPreferences.current = JSON.stringify(restored);
      setPreferences(restored);
      setReady(true);
    };
    void load().catch((error) => {
      if (active) setFailure(error);
    });
    return () => {
      active = false;
    };
  }, [native, persist]);

  useEffect(() => {
    const sample = () => {
      setSystemAppearance(native?.windowAppearance?.());
      setReducedMotion(native?.reducedMotion?.() ?? false);
    };
    sample();
    const timer = setInterval(sample, 500);
    return () => clearInterval(timer);
  }, [native]);

  useEffect(() => {
    if (!ready) return;
    native?.setWindowAppearance?.(preferences.preference);
    native?.setRemSize?.(preferences.uiFontSize);
    const serialized = JSON.stringify(preferences);
    if (serialized === lastPreferences.current) return;
    void persist
      .write(THEME_PERSIST_KEY, preferences)
      .then(() => {
        lastPreferences.current = serialized;
      })
      .catch((error) => errorHandler.current(error));
  }, [native, persist, preferences, ready]);

  const setPreference = useCallback((preference: ThemePreference) => {
    setPreferences((current) => ({ ...current, preference }));
  }, []);
  const setUiFontSize = useCallback((value: number) => {
    setPreferences((current) => ({
      ...current,
      uiFontSize: clampFontSize(value, DEFAULT_UI_FONT_SIZE),
    }));
  }, []);
  const setCodeFontSize = useCallback((value: number) => {
    setPreferences((current) => ({
      ...current,
      codeFontSize: clampFontSize(value, DEFAULT_CODE_FONT_SIZE),
    }));
  }, []);
  const value = useMemo<ThemeValue>(
    () => ({
      ...preferences,
      colors: (() => {
        const palette =
          resolveAppearance(preferences.preference, systemAppearance) === 'dark'
            ? darkColors
            : lightColors;
        return process.platform === 'darwin' ? { ...palette, sidebar: 'rgba(0,0,0,0)' } : palette;
      })(),
      sp: (size) => (size * preferences.uiFontSize) / DEFAULT_UI_FONT_SIZE,
      setPreference,
      setUiFontSize,
      setCodeFontSize,
      reducedMotion: motionOverride ?? reducedMotion,
    }),
    [
      preferences,
      systemAppearance,
      setPreference,
      setUiFontSize,
      setCodeFontSize,
      motionOverride,
      reducedMotion,
    ],
  );

  if (failure) throw failure;
  if (!ready) return null;
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeValue {
  const theme = useContext(ThemeContext);
  if (!theme) throw new Error('useTheme requires ThemeProvider');
  return theme;
}
