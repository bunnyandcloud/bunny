/** Copy text — Clipboard API when available, execCommand fallback for HTTP / older browsers. */
export function copyToClipboard(text: string): Promise<boolean> {
  if (typeof document === 'undefined') {
    return Promise.resolve(false);
  }

  const fallback = (): boolean => {
    try {
      const textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.setAttribute('readonly', '');
      textarea.style.position = 'fixed';
      textarea.style.left = '-9999px';
      textarea.style.top = '0';
      textarea.style.opacity = '0';
      document.body.appendChild(textarea);
      textarea.focus();
      textarea.select();
      textarea.setSelectionRange(0, text.length);
      const ok = document.execCommand('copy');
      document.body.removeChild(textarea);
      return ok;
    } catch {
      return false;
    }
  };

  if (navigator.clipboard?.writeText) {
    return navigator.clipboard
      .writeText(text)
      .then(() => true)
      .catch(() => fallback());
  }

  return Promise.resolve(fallback());
}
