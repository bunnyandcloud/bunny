import { create } from 'zustand';
import * as api from '../lib/api';

interface AuthState {
  user: { id: string; email: string; isOwner: boolean } | null;
  loading: boolean;
  check: () => Promise<void>;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
}

export const useAuth = create<AuthState>((set) => ({
  user: null,
  loading: true,
  check: async () => {
    try {
      const u = await api.me();
      set({
        user: { id: u.user_id, email: u.email, isOwner: u.is_owner },
        loading: false,
      });
    } catch {
      set({ user: null, loading: false });
    }
  },
  login: async (email, password) => {
    const u = await api.login(email, password);
    const me = await api.me();
    set({
      user: { id: u.user_id, email: u.email, isOwner: me.is_owner },
    });
  },
  logout: async () => {
    await fetch('/api/v1/auth/logout', { method: 'POST', credentials: 'include' });
    set({ user: null });
  },
}));
