import { create } from 'zustand';
import * as api from '../lib/api';

interface AuthState {
  user: { id: string; email: string } | null;
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
      set({ user: { id: u.user_id, email: u.email }, loading: false });
    } catch {
      set({ user: null, loading: false });
    }
  },
  login: async (email, password) => {
    const u = await api.login(email, password);
    set({ user: { id: u.user_id, email: u.email } });
  },
  logout: async () => {
    await fetch('/api/v1/auth/logout', { method: 'POST', credentials: 'include' });
    set({ user: null });
  },
}));
