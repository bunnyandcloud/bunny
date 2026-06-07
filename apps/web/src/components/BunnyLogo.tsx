import logoUrl from '../assets/logo.png';

interface Props {
  className?: string;
}

export default function BunnyLogo({ className = 'w-44 max-w-[220px]' }: Props) {
  return (
    <div className="flex flex-col items-center gap-2">
      <img
        src={logoUrl}
        alt=""
        aria-hidden
        className={`mx-auto block ${className}`}
      />
    </div>
  );
}
