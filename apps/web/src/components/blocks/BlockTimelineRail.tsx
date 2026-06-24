import type { AuthorSource } from '../../lib/api';
import discordIcon from '../../assets/discord-icon.png';

interface Props {
  timestamp: string;
  authorSource?: AuthorSource;
}

function formatTimelineDate(timestamp: string): string {
  return timestamp.slice(0, 10).replaceAll('-', '/');
}

export default function BlockTimelineRail({ timestamp, authorSource }: Props) {
  const date = formatTimelineDate(timestamp);
  const time = timestamp.slice(11, 19);
  return (
    <div className="flex w-16 shrink-0 flex-col items-end gap-0.5 pr-2 pt-1 text-[10px] text-bunny-muted font-mono">
      <span className="leading-none">{date}</span>
      <span className="leading-none">{time}</span>
      {authorSource === 'discord' ? (
        <img
          src={discordIcon}
          alt=""
          className="mt-0.5 h-4 w-4 shrink-0"
          aria-hidden
        />
      ) : null}
      <span className="mt-1 h-full w-px bg-bunny-border/60" aria-hidden />
    </div>
  );
}
