import { create } from 'zustand';
import {
  applyTheme,
  resolveInitialTheme,
  writeStoredTheme,
  type UiTheme,
} from '../lib/theme';
import { useTerminalTheme } from './terminalTheme';

interface ThemeState {
  theme: UiTheme;
  setTheme: (theme: UiTheme) => void;
}

const initialTheme = resolveInitialTheme();
applyTheme(initialTheme);

export const useTheme = create<ThemeState>((set) => ({
  theme: initialTheme,
  setTheme: (theme) => {
    writeStoredTheme(theme);
    applyTheme(theme);
    set({ theme });
    useTerminalTheme.getState().syncWithUiTheme(theme);
  },
}));
