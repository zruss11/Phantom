import { create } from 'zustand';

interface LayoutStore {
  sidebarCollapsed: boolean;
  rightPanelOpen: boolean;
  toggleSidebar: () => void;
  toggleRightPanel: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setRightPanelOpen: (open: boolean) => void;
}

export const useLayoutStore = create<LayoutStore>((set) => ({
  sidebarCollapsed: false,
  rightPanelOpen: false,
  toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
  toggleRightPanel: () => set((state) => ({ rightPanelOpen: !state.rightPanelOpen })),
  setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),
  setRightPanelOpen: (open) => set({ rightPanelOpen: open }),
}));
