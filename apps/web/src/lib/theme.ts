export type UiTheme = 'dark' | 'light';

export const THEME_STORAGE_KEY = 'bunny.ui.theme';

export function readStoredTheme(): UiTheme | null {
  try {
    const v = localStorage.getItem(THEME_STORAGE_KEY);
    return v === 'light' || v === 'dark' ? v : null;
  } catch {
    return null;
  }
}

export function writeStoredTheme(theme: UiTheme) {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    /* ignore */
  }
}

export function resolveInitialTheme(): UiTheme {
  const stored = readStoredTheme();
  if (stored) return stored;
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: light)').matches) {
    return 'light';
  }
  return 'dark';
}

export function applyTheme(theme: UiTheme) {
  document.documentElement.classList.toggle('light', theme === 'light');
  document.documentElement.style.colorScheme = theme;
}
