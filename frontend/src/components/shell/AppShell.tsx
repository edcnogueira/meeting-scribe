'use client';

import React from 'react';
import { ShellStateProvider, useShell } from './ShellContext';
import { ShellSidebar } from './ShellSidebar';

/**
 * Redesign application shell (task R1): the 264px folder-tree sidebar plus the
 * main content region. R2–R4 screens render as `children` inside `.main` and
 * compose their own header from `ScreenHeader`. Sidebar collapse/restore is
 * driven by the shell state (`.app.sidebar-hidden`).
 */
function ShellFrame({ children }: { children: React.ReactNode }) {
  const { sidebarHidden } = useShell();
  return (
    <div className={`app${sidebarHidden ? ' sidebar-hidden' : ''}`}>
      <ShellSidebar />
      <div className="main">{children}</div>
    </div>
  );
}

export function AppShell({ children }: { children: React.ReactNode }) {
  return (
    <ShellStateProvider>
      <ShellFrame>{children}</ShellFrame>
    </ShellStateProvider>
  );
}
