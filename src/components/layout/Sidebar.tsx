import {
  Plus,
  ClipboardList,
  BarChart3,
  GitBranch,
  Github,
  Gauge,
  Bug,
  StickyNote,
  Zap,
  Terminal,
  KeyRound,
  Settings,
  PanelLeftClose,
  PanelLeft,
  Sun,
  Moon,
} from 'lucide-react';
import { SidebarSection, SidebarItem } from './SidebarNav';
import { useLayoutStore } from '../../stores/layout';
import { useThemeStore } from '../../stores/theme';
import clsx from 'clsx';

export function Sidebar() {
  const collapsed = useLayoutStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar);
  const { resolved, setMode } = useThemeStore();

  const toggleTheme = () => {
    setMode(resolved === 'dark' ? 'light' : 'dark');
  };

  return (
    <aside
      className={clsx(
        'flex h-full flex-col border-r border-border-subtle bg-bg-sidebar transition-all duration-200 ease-out',
        collapsed ? 'w-12' : 'w-60'
      )}
    >
      {/* Navigation */}
      <nav className="flex-1 overflow-y-auto overflow-x-hidden px-1.5 py-2">
        <SidebarSection label="Workspace" />
        <SidebarItem icon={Plus} label="New Task" to="/tasks?new=true" />
        <SidebarItem icon={ClipboardList} label="Tasks" to="/tasks" />
        <SidebarItem icon={BarChart3} label="Analytics" to="/analytics" />
        <SidebarItem icon={GitBranch} label="Review" to="/review" />

        <SidebarSection label="Integrations" />
        <SidebarItem icon={Github} label="GitHub" to="/integrations/github" />
        <SidebarItem icon={Gauge} label="Linear" to="/integrations/linear" />
        <SidebarItem icon={Bug} label="Sentry" to="/integrations/sentry" />

        <SidebarSection label="Tools" />
        <SidebarItem icon={StickyNote} label="Notes" to="/notes" />
        <SidebarItem icon={Zap} label="Skills" to="/skills" />
        <SidebarItem icon={Terminal} label="Terminal" to="/terminal" />
      </nav>

      {/* Bottom section */}
      <div className="border-t border-border-subtle px-1.5 py-2">
        <SidebarItem icon={KeyRound} label="Accounts" to="/accounts" />
        <SidebarItem icon={Settings} label="Settings" to="/settings" />

        {/* Theme toggle */}
        <button
          onClick={toggleTheme}
          className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] font-medium text-text-secondary transition-colors hover:bg-bg-surface-hover hover:text-text-primary"
          title={collapsed ? (resolved === 'dark' ? 'Light mode' : 'Dark mode') : undefined}
        >
          {resolved === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
          {!collapsed && <span>{resolved === 'dark' ? 'Light mode' : 'Dark mode'}</span>}
        </button>

        {/* Collapse toggle */}
        <button
          onClick={toggleSidebar}
          className="flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] font-medium text-text-secondary transition-colors hover:bg-bg-surface-hover hover:text-text-primary"
          title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        >
          {collapsed ? <PanelLeft size={16} /> : <PanelLeftClose size={16} />}
          {!collapsed && <span>Collapse</span>}
        </button>
      </div>
    </aside>
  );
}
