import type { SecretMeta } from '../lib/api';

interface Props {
  secrets: SecretMeta[];
  locked: boolean;
  onInject: (envVar: string) => void;
  onUnlock: () => void;
  onManage: () => void;
}

export default function VaultInjectPanel({
  secrets,
  locked,
  onInject,
  onUnlock,
  onManage,
}: Props) {
  return (
    <div className="h-full flex flex-col bg-bunny-panel min-w-0">
      <div className="px-2 py-1 border-b border-bunny-border shrink-0 flex items-center justify-between gap-2">
        <div className="min-w-0">
          <h2 className="text-xs uppercase tracking-wide text-bunny-muted">Vault</h2>
          <p className="text-[10px] text-bunny-muted truncate">
            {locked ? 'Verrouillé' : 'Clic → insère au curseur'}
          </p>
        </div>
        <button
          type="button"
          onClick={onManage}
          className="text-[10px] text-bunny-muted hover:text-bunny-accent shrink-0"
        >
          Gérer →
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-2 min-h-0">
        {locked ? (
          <div className="flex items-center gap-2">
            <span className="text-xs text-orange-300/90">Locked</span>
            <button
              type="button"
              onClick={onUnlock}
              className="text-xs px-2 py-1 rounded bg-bunny-accent text-bunny-bg font-medium hover:opacity-90"
            >
              Unlock
            </button>
          </div>
        ) : secrets.length === 0 ? (
          <p className="text-xs text-bunny-muted">Aucun secret.</p>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {secrets.map((secret) => (
              <button
                key={`${secret.name}:${secret.scope}:${secret.session_id ?? ''}`}
                type="button"
                title={`Insérer $${secret.env_var} au curseur`}
                onClick={() => onInject(secret.env_var)}
                className="px-2 py-1 rounded border border-bunny-border hover:border-bunny-accent hover:bg-bunny-bg font-mono text-[11px] text-bunny-accent"
              >
                ${secret.env_var}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
