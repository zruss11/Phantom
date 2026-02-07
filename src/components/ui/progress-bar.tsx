import clsx from 'clsx';

interface ProgressBarProps {
  value: number;
  max?: number;
  color?: string;
  className?: string;
}

export function ProgressBar({ value, max = 100, color, className }: ProgressBarProps) {
  const pct = Math.min(100, Math.max(0, (value / max) * 100));
  return (
    <div className={clsx('h-1.5 w-full overflow-hidden rounded-full bg-bg-surface', className)}>
      <div
        className="h-full rounded-full transition-all duration-300 ease-out"
        style={{ width: `${pct}%`, backgroundColor: color || 'var(--accent-primary)' }}
      />
    </div>
  );
}
