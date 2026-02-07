import { Outlet } from 'react-router-dom';
import { Sidebar } from './Sidebar';
import { WindowControls } from './WindowControls';
import { useLayoutStore } from '../../stores/layout';

export function AppShell() {
  const sidebarCollapsed = useLayoutStore((s) => s.sidebarCollapsed);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-bg-base">
      <WindowControls />
      <div
        className="flex flex-1 overflow-hidden"
        style={{
          display: 'grid',
          gridTemplateColumns: `${sidebarCollapsed ? '48px' : '240px'} 1fr`,
          transition: 'grid-template-columns 200ms ease-out',
        }}
      >
        <Sidebar />
        <main className="flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
