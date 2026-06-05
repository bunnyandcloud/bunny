import baseMessages from '@bunny/i18n/messages.json';
import webMessages from '@bunny/i18n/web-messages.json';
import { useCallback } from 'react';
import { useAuth } from './store/auth';

export type UiLocale = 'en' | 'fr';

export const LOCALE_STORAGE_KEY = 'bunny.ui.locale';

type MessageEntry = { en: string; fr: string };

const catalog: Record<string, MessageEntry> = {
  ...(baseMessages as Record<string, MessageEntry>),
  ...(webMessages as Record<string, MessageEntry>),
};

export function parseUiLocale(s: string | undefined): UiLocale {
  if (s === 'fr') return 'fr';
  return 'en';
}

export function readStoredLocale(): UiLocale | null {
  try {
    const v = localStorage.getItem(LOCALE_STORAGE_KEY);
    return v === 'fr' || v === 'en' ? v : null;
  } catch {
    return null;
  }
}

export function writeStoredLocale(locale: UiLocale) {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  } catch {
    /* ignore */
  }
}

export function guessBrowserLocale(): UiLocale {
  const lang = navigator.language.toLowerCase();
  return lang.startsWith('fr') ? 'fr' : 'en';
}

export function effectiveLocale(
  userLocale: UiLocale | undefined,
): UiLocale {
  return userLocale ?? readStoredLocale() ?? guessBrowserLocale();
}

export function t(
  locale: UiLocale,
  key: string,
  vars?: Record<string, string>,
): string {
  const entry = catalog[key];
  if (!entry) {
    console.warn(`missing i18n key: ${key}`);
    return key;
  }
  let out = entry[locale] ?? entry.en;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      out = out.replaceAll(`{${name}}`, value);
    }
  }
  return out;
}

/** Hook: translate using the current user (or guest) locale. */
export function useT() {
  const locale = useAuth((s) => s.effectiveLocale());
  return useCallback(
    (key: string, vars?: Record<string, string>) => t(locale, key, vars),
    [locale],
  );
}

export function useLocale(): UiLocale {
  return useAuth((s) => s.effectiveLocale());
}
