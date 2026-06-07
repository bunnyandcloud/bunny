import { t, type UiLocale } from '../i18n';
import { useAuth } from '../store/auth';

interface Props {
  className?: string;
}

/** Compact language picker (EN/FR) for the top bar. */
export default function LanguageSelect({ className = '' }: Props) {
  const locale = useAuth((s) => s.effectiveLocale());
  const setLocale = useAuth((s) => s.setLocale);
  const busy = useAuth((s) => s.localeBusy);

  return (
    <select
      aria-label={t(locale, 'web.home.language_label')}
      value={locale}
      disabled={busy}
      onChange={(e) => {
        void setLocale(e.target.value as UiLocale);
      }}
      className={
        className ||
        'text-xs px-1.5 py-0.5 rounded border border-bunny-border bg-bunny-bg text-bunny-muted hover:text-bunny-fg cursor-pointer disabled:opacity-50'
      }
    >
      <option value="en">{t(locale, 'web.home.language_en')}</option>
      <option value="fr">{t(locale, 'web.home.language_fr')}</option>
    </select>
  );
}
