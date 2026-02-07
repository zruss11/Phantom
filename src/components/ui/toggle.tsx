import clsx from 'clsx';

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, disabled }: ToggleProps) {
  return (
    <label className={clsx('inline-flex items-center gap-2.5 cursor-pointer', disabled && 'opacity-50 cursor-not-allowed')}>
      <button
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={clsx(
          'relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors',
          checked ? 'bg-accent-primary' : 'bg-border-strong',
        )}
      >
        <span className={clsx('inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform', checked ? 'translate-x-4' : 'translate-x-0.5')} />
      </button>
      {label && <span className="text-sm text-text-primary">{label}</span>}
    </label>
  );
}
