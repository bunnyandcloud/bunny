interface Props {
  onUnlock: () => void;
  compact?: boolean;
}

export default function SecretsVaultBanner({ onUnlock, compact }: Props) {
  return (
    <div
      className={
        compact
          ? 'px-3 py-2 text-xs border-b border-orange-400/30 bg-orange-400/10 text-orange-200/90'
          : 'px-4 py-2.5 text-sm border-b border-orange-400/30 bg-orange-400/10 text-orange-100 flex flex-wrap items-center justify-center gap-x-2 gap-y-1'
      }
      role="status"
    >
      <span>
        Secrets vault locked — les terminaux n&apos;ont pas accès aux secrets.
      </span>
      <button
        type="button"
        onClick={onUnlock}
        className="text-bunny-accent font-medium hover:underline shrink-0"
      >
        Unlock
      </button>
    </div>
  );
}
