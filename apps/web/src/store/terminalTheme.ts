import { create } from 'zustand';
import { resolveInitialTheme, type UiTheme } from '../lib/theme';
import {
  migrateTerminalThemeCustomFlag,
  resolveTerminalTheme,
  setUserTerminalTheme,
  type TerminalThemeId,
} from '../lib/terminalThemes';

interface TerminalThemeState {
  themeId: TerminalThemeId;
  setThemeId: (themeId: TerminalThemeId) => void;
  syncWithUiTheme: (uiTheme: UiTheme) => void;
}

migrateTerminalThemeCustomFlag();
const initialUiTheme = resolveInitialTheme();

export const useTerminalTheme = create<TerminalThemeState>((set) => ({
  themeId: resolveTerminalTheme(initialUiTheme),
  setThemeId: (themeId) => {
    setUserTerminalTheme(themeId);
    set({ themeId });
  },
  syncWithUiTheme: (uiTheme) => {
    set({ themeId: resolveTerminalTheme(uiTheme) });
  },
}));
