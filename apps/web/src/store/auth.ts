import { create } from 'zustand';
import * as api from '../lib/api';

interface AuthState {
  user: {
    id: string;
    email: string;
    isOwner: boolean;
    mfaEnabled: boolean;
    canCreateSessions: boolean;
  } | null;
  loading: boolean;
  check: () => Promise<void>;
  login: (email: string, password: string) => Promise<api.LoginResponse>;
  completeMfa: (code: string, mfaChallengeToken: string) => Promise<void>;
  logout: () => Promise<void>;
}

export const useAuth = create<AuthState>((set) => ({
  user: null,
  loading: true,
  check: async () => {
    try {
      const u = await api.me();
      set({
        user: {
          id: u.user_id,
          email: u.email,
          isOwner: u.is_owner,
          mfaEnabled: u.mfa_enabled,
          canCreateSessions: u.can_create_sessions,
        },
        loading: false,
      });
    } catch {
      set({ user: null, loading: false });
    }
  },
  login: async (email, password) => {
    const result = await api.login(email, password);
    if (!result.mfa_required) {
      const me = await api.me();
      set({
        user: {
          id: result.user_id,
          email: result.email,
          isOwner: me.is_owner,
          mfaEnabled: me.mfa_enabled,
          canCreateSessions: me.can_create_sessions,
        },
      });
    }
    return result;
  },
  completeMfa: async (code, mfaChallengeToken) => {
    const result = await api.verifyMfa(code, mfaChallengeToken);
    // Establish session immediately from verify response (do not block on /auth/me).
    set({
      user: {
        id: result.user_id,
        email: result.email,
        isOwner: false,
        mfaEnabled: true,
        canCreateSessions: false,
      },
    });
    try {
      const me = await api.me();
      set({
        user: {
          id: me.user_id,
          email: me.email,
          isOwner: me.is_owner,
          mfaEnabled: me.mfa_enabled,
          canCreateSessions: me.can_create_sessions,
        },
      });
    } catch {
      // Cookie may lag in rare cases; user can still proceed.
    }
  },
  logout: async () => {
    await fetch('/api/v1/auth/logout', {
      method: 'POST',
      credentials: 'include',
    });
    set({ user: null, loading: false });
  },
}));
