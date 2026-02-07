import { create } from 'zustand';
import type { ThemeMode } from '../lib/types';

interface ThemeStore {
  mode: ThemeMode;
  resolved: 'dark' | 'light';
  setMode: (mode: ThemeMode) => void;
}

function getSystemTheme(): 'dark' | 'light' {
  if (typeof window === 'undefined') return 'dark';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function resolveTheme(mode: ThemeMode): 'dark' | 'light' {
  if (mode === 'system') return getSystemTheme();
  return mode;
}

function getInitialMode(): ThemeMode {
  if (typeof window === 'undefined') return 'dark';
  const stored = localStorage.getItem('phantom-theme');
  if (stored === 'dark' || stored === 'light' || stored === 'system') return stored;
  return 'dark';
}

const initialMode = getInitialMode();

export const useThemeStore = create<ThemeStore>((set) => ({
  mode: initialMode,
  resolved: resolveTheme(initialMode),
  setMode: (mode) => {
    localStorage.setItem('phantom-theme', mode);
    const resolved = resolveTheme(mode);
    document.documentElement.setAttribute('data-theme', resolved);
    set({ mode, resolved });
  },
}));

// Apply initial theme on load
if (typeof window !== 'undefined') {
  document.documentElement.setAttribute('data-theme', resolveTheme(initialMode));

  // Listen for system theme changes when mode is 'system'
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    const { mode } = useThemeStore.getState();
    if (mode === 'system') {
      const resolved = getSystemTheme();
      document.documentElement.setAttribute('data-theme', resolved);
      useThemeStore.setState({ resolved });
    }
  });
}
