import { useT } from '../i18n';
import { TERMINAL_THEMES } from '../lib/terminalThemes';
import { useTerminalTheme } from '../store/terminalTheme';
import type { TerminalThemeId } from '../lib/terminalThemes';

interface Props {
  className?: string;
}

export default function TerminalThemeSelect({ className = '' }: Props) {
  const tr = useT();
  const themeId = useTerminalTheme((s) => s.themeId);
  const setThemeId = useTerminalTheme((s) => s.setThemeId);

  return (
    <select
      aria-label={tr('web.terminalTheme.label')}
      value={themeId}
      onChange={(e) => setThemeId(e.target.value as TerminalThemeId)}
      className={
        className ||
        'text-xs font-medium px-1.5 py-0.5 rounded border border-bunny-border bg-bunny-bg text-bunny-fg cursor-pointer'
      }
    >
      {TERMINAL_THEMES.map((option) => (
        <option key={option.id} value={option.id}>
          {tr(option.labelKey)}
        </option>
      ))}
    </select>
  );
}
