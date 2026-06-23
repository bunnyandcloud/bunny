import { useCallback, useId, useRef, useState } from 'react';
import type { AuthorSource, TerminalBlock } from '../../lib/api';
import { useT } from '../../i18n';
import { useTheme } from '../../store/theme';

interface Props {
  block: TerminalBlock;
}

type TipPos = { x: number; y: number };

function authorColorKey(block: TerminalBlock): string {
  return block.author_user_id ?? `${block.author_source}:${block.author_display}`;
}

function authorPillColors(
  key: string,
  light: boolean,
): { backgroundColor: string; color: string; borderColor?: string } {
  let hash = 0;
  for (let i = 0; i < key.length; i++) {
    hash = key.charCodeAt(i) + ((hash << 5) - hash);
  }
  const hue = Math.abs(hash) % 360;
  if (light) {
    return {
      backgroundColor: `hsl(${hue}, 48%, 86%)`,
      color: `hsl(${hue}, 62%, 24%)`,
      borderColor: `hsl(${hue}, 38%, 72%)`,
    };
  }
  return {
    backgroundColor: `hsla(${hue}, 58%, 52%, 0.3)`,
    color: `hsl(${hue}, 72%, 84%)`,
  };
}

function authorInitial(block: TerminalBlock): string {
  const fromGit = block.author_git_name?.trim() || block.author_display.trim();
  const letter = fromGit.match(/[A-Za-z0-9]/)?.[0];
  return letter ? letter.toUpperCase() : '?';
}

export default function BlockAuthorBadge({ block }: Props) {
  const tr = useT();
  const uiTheme = useTheme((s) => s.theme);
  const tipId = useId();
  const anchorRef = useRef<HTMLSpanElement>(null);
  const [tipPos, setTipPos] = useState<TipPos | null>(null);

  const sourceLabel: Record<AuthorSource, string> = {
    web: tr('web.notebook.authorSourceWeb'),
    discord: tr('web.notebook.authorSourceDiscord'),
    system: tr('web.notebook.authorSourceSystem'),
  };

  const gitName =
    block.author_git_name?.trim() ||
    block.author_display.trim() ||
    tr('web.notebook.authorNotConfigured');
  const gitEmail =
    block.author_git_email?.trim() || tr('web.notebook.authorNotConfigured');
  const colors = authorPillColors(authorColorKey(block), uiTheme === 'light');
  const initial = authorInitial(block);
  const pillStyle = {
    backgroundColor: colors.backgroundColor,
    color: colors.color,
    borderWidth: colors.borderColor ? 1 : undefined,
    borderStyle: colors.borderColor ? ('solid' as const) : undefined,
    borderColor: colors.borderColor,
  };

  const showTip = useCallback(() => {
    const el = anchorRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setTipPos({ x: rect.left + rect.width / 2, y: rect.top - 8 });
  }, []);

  const hideTip = useCallback(() => setTipPos(null), []);

  return (
    <>
      <span
        ref={anchorRef}
        tabIndex={0}
        role="button"
        aria-describedby={tipPos ? tipId : undefined}
        aria-label={`${gitName} · ${gitEmail}`}
        className="inline-flex h-6 min-w-6 shrink-0 items-center justify-center rounded-full px-1.5 text-[10px] font-bold leading-none outline-none ring-bunny-accent/60 transition-transform hover:scale-105 focus-visible:ring-2"
        style={pillStyle}
        onMouseEnter={showTip}
        onMouseLeave={hideTip}
        onFocus={showTip}
        onBlur={hideTip}
      >
        {initial}
      </span>
      {tipPos ? (
        <span
          id={tipId}
          role="tooltip"
          className="pointer-events-none fixed z-50 w-max max-w-[min(18rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-full rounded-md border border-bunny-border bg-bunny-panel px-3 py-2 text-left text-xs text-bunny-fg shadow-lg"
          style={{ left: tipPos.x, top: tipPos.y }}
        >
          <p className="font-medium text-bunny-fg">{gitName}</p>
          <p className="mt-0.5 font-mono text-[11px] text-bunny-muted">{gitEmail}</p>
          <p className="mt-1.5 border-t border-bunny-border/60 pt-1.5 text-[10px] uppercase tracking-wide text-bunny-muted">
            {sourceLabel[block.author_source]}
          </p>
        </span>
      ) : null}
    </>
  );
}
