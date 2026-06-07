import { useT } from '../i18n';
import { useTheme } from '../store/theme';
import type { UiTheme } from '../lib/theme';

interface Props {
  className?: string;
}

/** Compact light/dark picker for the top bar. */
export default function ThemeSelect({ className = '' }: Props) {
  const tr = useT();
  const theme = useTheme((s) => s.theme);
  const setTheme = useTheme((s) => s.setTheme);

  return (
    <select
      aria-label={tr('web.home.theme_label')}
      value={theme}
      onChange={(e) => setTheme(e.target.value as UiTheme)}
      className={
        className ||
        'text-xs px-1.5 py-0.5 rounded border border-bunny-border bg-bunny-bg text-bunny-muted hover:text-bunny-fg cursor-pointer'
      }
    >
      <option value="dark">{tr('web.home.theme_dark')}</option>
      <option value="light">{tr('web.home.theme_light')}</option>
    </select>
  );
}
