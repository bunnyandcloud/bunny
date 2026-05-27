import LogoutButton from './LogoutButton';
import MfaSettingsPanel from './MfaSettingsPanel';

interface Props {
  email: string;
}

export default function SecurityPage({ email }: Props) {
  return (
    <div className="min-h-screen flex flex-col items-center p-6">
      <div className="w-full max-w-lg flex items-center justify-between mb-8">
        <button
          type="button"
          onClick={() => { location.href = '/'; }}
          className="text-sm text-bunny-muted hover:text-gray-200"
        >
          ← Back
        </button>
        <h1 className="text-xl text-bunny-accent font-bold">Security</h1>
        <LogoutButton />
      </div>
      <MfaSettingsPanel email={email} />
    </div>
  );
}
