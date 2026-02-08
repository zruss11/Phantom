import { type LucideIcon } from 'lucide-react';
import clsx from 'clsx';

interface EmptyStateProps {
  icon: LucideIcon;
  title: string;
  description: string;
  className?: string;
  children?: React.ReactNode;
}

export function EmptyState({ icon: Icon, title, description, className, children }: EmptyStateProps) {
  return (
    <div className={clsx('flex flex-col items-center justify-center gap-3 p-8 text-center', className)}>
      <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-bg-surface">
        <Icon size={24} className="text-text-tertiary" />
      </div>
      <div>
        <h3 className="text-sm font-medium text-text-primary">{title}</h3>
        <p className="mt-1 text-xs text-text-secondary">{description}</p>
      </div>
      {children}
    </div>
  );
}
