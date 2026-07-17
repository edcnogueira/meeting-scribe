'use client';

import React, { createContext, useContext, useCallback, useEffect, useState } from 'react';

const HIDDEN_KEY = 'ms-sidebar-hidden';

interface ShellContextValue {
  /** Whether the sidebar is collapsed off-screen. */
  sidebarHidden: boolean;
  hideSidebar: () => void;
  showSidebar: () => void;
  toggleSidebar: () => void;
}

const ShellContext = createContext<ShellContextValue | null>(null);

/**
 * Holds the shared shell chrome state (sidebar collapse/restore) for the
 * redesign (task R1). Persisted so the collapsed state survives reloads.
 */
export function ShellStateProvider({ children }: { children: React.ReactNode }) {
  const [sidebarHidden, setSidebarHidden] = useState(false);

  useEffect(() => {
    try {
      setSidebarHidden(window.localStorage.getItem(HIDDEN_KEY) === '1');
    } catch {
      /* ignore */
    }
  }, []);

  const persist = useCallback((v: boolean) => {
    try {
      window.localStorage.setItem(HIDDEN_KEY, v ? '1' : '0');
    } catch {
      /* ignore */
    }
  }, []);

  const hideSidebar = useCallback(() => {
    setSidebarHidden(true);
    persist(true);
  }, [persist]);

  const showSidebar = useCallback(() => {
    setSidebarHidden(false);
    persist(false);
  }, [persist]);

  const toggleSidebar = useCallback(() => {
    setSidebarHidden((prev) => {
      persist(!prev);
      return !prev;
    });
  }, [persist]);

  return (
    <ShellContext.Provider value={{ sidebarHidden, hideSidebar, showSidebar, toggleSidebar }}>
      {children}
    </ShellContext.Provider>
  );
}

export function useShell(): ShellContextValue {
  const ctx = useContext(ShellContext);
  if (!ctx) {
    throw new Error('useShell must be used within a ShellStateProvider');
  }
  return ctx;
}
