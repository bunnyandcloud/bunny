interface Props {
  timestamp: string;
}

export default function BlockTimelineRail({ timestamp }: Props) {
  const time = timestamp.slice(11, 19);
  return (
    <div className="flex w-14 shrink-0 flex-col items-end pr-2 pt-1 text-[10px] text-bunny-muted font-mono">
      <span>{time}</span>
      <span className="mt-1 h-full w-px bg-bunny-border/60" aria-hidden />
    </div>
  );
}
