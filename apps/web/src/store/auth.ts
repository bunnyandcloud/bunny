import { create } from 'zustand';
import * as api from '../lib/api';
import {
  effectiveLocale,
  parseUiLocale,
  writeStoredLocale,
  type UiLocale,
} from '../i18n';

interface AuthState {
  user: {
    id: string;
    email: string;
    isOwner: boolean;
    mfaEnabled: boolean;
    canCreateSessions: boolean;
    locale: UiLocale;
  } | null;
  loading: boolean;
  localeBusy: boolean;
  effectiveLocale: () => UiLocale;
  check: () => Promise<void>;
  setLocale: (locale: UiLocale) => Promise<void>;
  login: (email: string, password: string) => Promise<api.LoginResponse>;
  completeMfa: (code: string, mfaChallengeToken: string) => Promise<void>;
  logout: () => Promise<void>;
}

function userFromMe(me: Awaited<ReturnType<typeof api.me>>) {
  const locale = parseUiLocale(me.locale);
  writeStoredLocale(locale);
  return {
    id: me.user_id,
    email: me.email,
    isOwner: me.is_owner,
    mfaEnabled: me.mfa_enabled,
    canCreateSessions: me.can_create_sessions,
    locale,
  };
}

function applyDocumentLang(locale: UiLocale) {
  document.documentElement.lang = locale;
}

export const useAuth = create<AuthState>((set, get) => ({
  user: null,
  loading: true,
  localeBusy: false,
  effectiveLocale: () => effectiveLocale(get().user?.locale),
  check: async () => {
    try {
      const u = await api.me();
      const user = userFromMe(u);
      applyDocumentLang(user.locale);
      set({ user, loading: false });
    } catch {
      applyDocumentLang(effectiveLocale(undefined));
      set({ user: null, loading: false });
    }
  },
  setLocale: async (locale) => {
    set({ localeBusy: true });
    try {
      writeStoredLocale(locale);
      applyDocumentLang(locale);
      if (get().user) {
        const me = await api.updateLocale(locale);
        const user = userFromMe(me);
        applyDocumentLang(user.locale);
        set({ user });
      } else {
        set((s) =>
          s.user ? { user: { ...s.user, locale } } : s,
        );
      }
    } finally {
      set({ localeBusy: false });
    }
  },
  login: async (email, password) => {
    const result = await api.login(email, password);
    if (!result.mfa_required) {
      const me = await api.me();
      const user = userFromMe(me);
      applyDocumentLang(user.locale);
      set({ user });
    }
    return result;
  },
  completeMfa: async (code, mfaChallengeToken) => {
    const result = await api.verifyMfa(code, mfaChallengeToken);
    const guest = effectiveLocale(undefined);
    set({
      user: {
        id: result.user_id,
        email: result.email,
        isOwner: false,
        mfaEnabled: true,
        canCreateSessions: false,
        locale: guest,
      },
    });
    try {
      const me = await api.me();
      const user = userFromMe(me);
      applyDocumentLang(user.locale);
      set({ user });
    } catch {
      applyDocumentLang(guest);
    }
  },
  logout: async () => {
    await fetch('/api/v1/auth/logout', {
      method: 'POST',
      credentials: 'include',
    });
    applyDocumentLang(effectiveLocale(undefined));
    set({ user: null, loading: false });
  },
}));
