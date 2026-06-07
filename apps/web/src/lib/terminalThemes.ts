import type { ITheme } from '@xterm/xterm';
import type { UiTheme } from './theme';

export type TerminalThemeId = 'bunny-dark' | 'bunny-light' | 'classic' | 'midnight' | 'paper';

export const TERMINAL_THEME_STORAGE_KEY = 'bunny.ui.terminalTheme';
export const TERMINAL_THEME_CUSTOM_KEY = 'bunny.ui.terminalThemeCustom';

export interface TerminalThemeOption {
  id: TerminalThemeId;
  labelKey: string;
  theme: ITheme;
}

export const TERMINAL_THEMES: TerminalThemeOption[] = [
  {
    id: 'bunny-dark',
    labelKey: 'web.terminalTheme.bunnyDark',
    theme: {
      background: '#0d1117',
      foreground: '#c9d1d9',
      cursor: '#9498ff',
      selectionBackground: '#9498ff44',
    },
  },
  {
    id: 'bunny-light',
    labelKey: 'web.terminalTheme.bunnyLight',
    theme: {
      background: '#f6f8fa',
      foreground: '#1f2328',
      cursor: '#7b83f0',
      selectionBackground: '#7b83f044',
    },
  },
  {
    id: 'classic',
    labelKey: 'web.terminalTheme.classic',
    theme: {
      background: '#000000',
      foreground: '#e6e6e6',
      cursor: '#9498ff',
      selectionBackground: '#9498ff44',
    },
  },
  {
    id: 'midnight',
    labelKey: 'web.terminalTheme.midnight',
    theme: {
      background: '#1a1b26',
      foreground: '#a9b1d6',
      cursor: '#9498ff',
      selectionBackground: '#9498ff44',
    },
  },
  {
    id: 'paper',
    labelKey: 'web.terminalTheme.paper',
    theme: {
      background: '#faf8f5',
      foreground: '#2d2a24',
      cursor: '#7b83f0',
      selectionBackground: '#7b83f044',
    },
  },
];

const themeById = Object.fromEntries(
  TERMINAL_THEMES.map((option) => [option.id, option]),
) as Record<TerminalThemeId, TerminalThemeOption>;

export function parseTerminalThemeId(value: string | null | undefined): TerminalThemeId {
  if (value && value in themeById) {
    return value as TerminalThemeId;
  }
  return 'bunny-dark';
}

export function readStoredTerminalTheme(): TerminalThemeId | null {
  try {
    const raw = localStorage.getItem(TERMINAL_THEME_STORAGE_KEY);
    if (!raw) return null;
    return parseTerminalThemeId(raw);
  } catch {
    return null;
  }
}

export function readTerminalThemeCustom(): boolean {
  try {
    return localStorage.getItem(TERMINAL_THEME_CUSTOM_KEY) === '1';
  } catch {
    return false;
  }
}

export function writeTerminalThemeCustom(custom: boolean) {
  try {
    if (custom) {
      localStorage.setItem(TERMINAL_THEME_CUSTOM_KEY, '1');
    } else {
      localStorage.removeItem(TERMINAL_THEME_CUSTOM_KEY);
    }
  } catch {
    /* ignore */
  }
}

/** One-time: a stored theme before custom tracking implies an explicit user choice. */
export function migrateTerminalThemeCustomFlag() {
  if (readStoredTerminalTheme() && !readTerminalThemeCustom()) {
    writeTerminalThemeCustom(true);
  }
}

export function writeStoredTerminalTheme(themeId: TerminalThemeId) {
  try {
    localStorage.setItem(TERMINAL_THEME_STORAGE_KEY, themeId);
  } catch {
    /* ignore */
  }
}

export function defaultTerminalThemeForUi(uiTheme: UiTheme): TerminalThemeId {
  return uiTheme === 'light' ? 'bunny-light' : 'bunny-dark';
}

export function resolveTerminalTheme(uiTheme: UiTheme): TerminalThemeId {
  if (readTerminalThemeCustom()) {
    return readStoredTerminalTheme() ?? defaultTerminalThemeForUi(uiTheme);
  }
  return defaultTerminalThemeForUi(uiTheme);
}

export function setUserTerminalTheme(themeId: TerminalThemeId) {
  writeStoredTerminalTheme(themeId);
  writeTerminalThemeCustom(true);
}

export function getTerminalTheme(themeId: TerminalThemeId): ITheme {
  return themeById[themeId].theme;
}
