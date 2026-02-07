import { Search, Minus, Square, X } from 'lucide-react';

// Tauri APIs are only available inside the Tauri runtime.
// In a plain browser (npm run dev without cargo tauri dev) they will be undefined.
const isTauri = typeof window !== 'undefined' && !!(window as any).__TAURI_INTERNALS__;

async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  if (!isTauri) return;
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  const win = getCurrentWindow();
  if (action === 'minimize') win.minimize();
  else if (action === 'toggleMaximize') win.toggleMaximize();
  else win.close();
}

export function WindowControls() {
  return (
    <div
      data-tauri-drag-region
      className="flex h-[38px] items-center justify-between border-b border-border-subtle bg-bg-sidebar px-3"
    >
      {/* Left: Logo */}
      <div className="flex items-center gap-2" data-tauri-drag-region>
        <span className="text-sm font-semibold text-text-primary" data-tauri-drag-region>
          Phantom
        </span>
      </div>

      {/* Center: Search trigger */}
      <button
        className="flex items-center gap-2 rounded-md border border-border-default bg-bg-surface px-3 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-surface-hover hover:text-text-primary"
        onClick={() => {/* Cmd+K palette - placeholder */}}
      >
        <Search size={14} />
        <span>Search</span>
        <kbd className="ml-2 rounded bg-bg-base px-1.5 py-0.5 text-[10px] font-medium text-text-tertiary">
          ⌘K
        </kbd>
      </button>

      {/* Right: Window controls */}
      {isTauri && (
        <div className="flex items-center gap-1">
          <button
            onClick={() => windowAction('minimize')}
            className="flex h-6 w-6 items-center justify-center rounded text-text-tertiary transition-colors hover:bg-bg-surface-hover hover:text-text-secondary"
          >
            <Minus size={14} />
          </button>
          <button
            onClick={() => windowAction('toggleMaximize')}
            className="flex h-6 w-6 items-center justify-center rounded text-text-tertiary transition-colors hover:bg-bg-surface-hover hover:text-text-secondary"
          >
            <Square size={12} />
          </button>
          <button
            onClick={() => windowAction('close')}
            className="flex h-6 w-6 items-center justify-center rounded text-text-tertiary transition-colors hover:bg-bg-surface-hover hover:text-red-400"
          >
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
