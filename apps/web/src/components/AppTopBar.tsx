import LanguageSelect from './LanguageSelect';
import LogoutButton from './LogoutButton';
import ThemeSelect from './ThemeSelect';

interface Props {
  /** Extra nodes between language and sign out (e.g. session actions). */
  children?: React.ReactNode;
  logoutClassName?: string;
  className?: string;
}

/** Top-right bar: language + sign out (+ optional actions). */
export default function AppTopBar({
  children,
  logoutClassName,
  className = 'flex items-center gap-2 shrink-0',
}: Props) {
  return (
    <div className={className}>
      {children}
      <ThemeSelect />
      <LanguageSelect />
      <LogoutButton className={logoutClassName} />
    </div>
  );
}
