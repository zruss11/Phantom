import { type LucideIcon } from 'lucide-react';
import { NavLink } from 'react-router-dom';
import clsx from 'clsx';
import { useLayoutStore } from '../../stores/layout';

interface SidebarSectionProps {
  label: string;
}

export function SidebarSection({ label }: SidebarSectionProps) {
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);
  if (collapsed) return <div className="my-2 border-t border-border-subtle" />;
  return (
    <div className="mb-1 mt-4 px-3">
      <span className="text-[11px] font-semibold uppercase tracking-wider text-text-tertiary">
        {label}
      </span>
    </div>
  );
}

interface SidebarItemProps {
  icon: LucideIcon;
  label: string;
  to: string;
}

export function SidebarItem({ icon: Icon, label, to }: SidebarItemProps) {
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);

  return (
    <NavLink
      to={to}
      className={({ isActive }) =>
        clsx(
          'group flex h-8 items-center gap-2.5 rounded-md px-2.5 text-[13px] font-medium transition-colors',
          isActive
            ? 'bg-accent-primary-muted text-accent-primary'
            : 'text-text-secondary hover:bg-bg-surface-hover hover:text-text-primary'
        )
      }
      title={collapsed ? label : undefined}
    >
      <Icon size={16} className="shrink-0" />
      {!collapsed && <span className="truncate">{label}</span>}
    </NavLink>
  );
}
