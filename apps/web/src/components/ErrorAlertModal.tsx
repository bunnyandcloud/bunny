import { useT } from '../i18n';

interface Props {
  open: boolean;
  message: string | null;
  title?: string;
  onClose: () => void;
}

export default function ErrorAlertModal({ open, message, title, onClose }: Props) {
  const tr = useT();
  if (!open || !message) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="error-alert-title"
      aria-describedby="error-alert-message"
      onClick={onClose}
    >
      <div
        className="w-full max-w-md rounded border border-red-500/40 bg-bunny-panel p-4 space-y-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 id="error-alert-title" className="text-sm font-medium text-red-300">
          {title ?? tr('web.error.title')}
        </h2>
        <p id="error-alert-message" className="text-sm text-gray-200 whitespace-pre-wrap break-words">
          {message}
        </p>
        <div className="flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1.5 text-sm rounded bg-bunny-accent text-white hover:opacity-90"
          >
            {tr('web.error.dismiss')}
          </button>
        </div>
      </div>
    </div>
  );
}
