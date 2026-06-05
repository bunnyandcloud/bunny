import { useT } from '../i18n';
import AppTopBar from './AppTopBar';
import MfaSettingsPanel from './MfaSettingsPanel';

interface Props {
  email: string;
}

export default function SecurityPage({ email }: Props) {
  const tr = useT();

  return (
    <div className="min-h-screen flex flex-col items-center p-6">
      <div className="w-full max-w-lg flex items-center justify-between mb-8 gap-2">
        <button
          type="button"
          onClick={() => { location.href = '/'; }}
          className="text-sm text-bunny-muted hover:text-gray-200"
        >
          {tr('web.common.back')}
        </button>
        <h1 className="text-xl text-bunny-accent font-bold">{tr('web.security.title')}</h1>
        <AppTopBar />
      </div>
      <MfaSettingsPanel email={email} />
    </div>
  );
}
