import { useMemo, useState } from 'react';
import { useT } from '../../i18n';

export const NOTEBOOK_OUTPUT_PREVIEW_LINES = 15;

interface Props {
  text: string;
  failed?: boolean;
}

function normalizeOutputText(text: string): string {
  return text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
}

export default function BlockOutput({ text, failed }: Props) {
  const tr = useT();
  const [expanded, setExpanded] = useState(false);
  const lines = useMemo(() => normalizeOutputText(text).split('\n'), [text]);
  const lineCount = lines.length;
  const hiddenLines = Math.max(0, lineCount - NOTEBOOK_OUTPUT_PREVIEW_LINES);
  const isTruncatable = hiddenLines > 0;
  const visibleText = useMemo(() => {
    if (!isTruncatable || expanded) {
      return lines.join('\n');
    }
    return lines.slice(0, NOTEBOOK_OUTPUT_PREVIEW_LINES).join('\n');
  }, [expanded, isTruncatable, lines]);

  const preClassName = [
    'notebook-output-pre whitespace-pre-wrap break-words font-mono text-xs leading-normal',
    failed ? 'text-red-400/90' : 'text-bunny-fg/90',
    !expanded && isTruncatable ? 'notebook-output-pre--collapsed' : '',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className="ml-8 space-y-1">
      <pre className={preClassName}>{visibleText}</pre>
      {isTruncatable ? (
        <button
          type="button"
          className="rounded border border-bunny-border px-2 py-0.5 text-xs text-bunny-muted hover:bg-bunny-panel hover:text-bunny-fg"
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded
            ? tr('web.notebook.showLess')
            : tr('web.notebook.readMore', { count: String(hiddenLines) })}
        </button>
      ) : null}
    </div>
  );
}
