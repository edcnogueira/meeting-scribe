'use client';

import React from 'react';
import { useShell } from './ShellContext';
import { PanelIcon } from './icons';

export interface Crumb {
  label: string;
  href?: string;
}

interface ScreenHeaderProps {
  /** Breadcrumb trail. The last entry renders as the current location. */
  crumbs?: Crumb[];
  /** Right-aligned actions (e.g. "Reveal in Finder"). */
  actions?: React.ReactNode;
  className?: string;
}

/**
 * Standard screen header for the redesign (task R1): a show-sidebar button
 * (visible only while the sidebar is collapsed), breadcrumbs, and a right-aligned
 * actions slot. R2–R4 screens compose their headers from this piece.
 */
export function ScreenHeader({ crumbs = [], actions, className }: ScreenHeaderProps) {
  const { sidebarHidden, showSidebar } = useShell();
  const last = crumbs.length - 1;

  return (
    <header className={`header ${className ?? ''}`}>
      {sidebarHidden && (
        <button
          type="button"
          className="icon-btn js-show-sidebar"
          onClick={showSidebar}
          data-tip="Mostrar barra lateral"
          aria-label="Show sidebar"
        >
          <PanelIcon />
        </button>
      )}

      <nav className="crumbs" aria-label="Breadcrumb">
        {crumbs.map((c, i) => (
          <React.Fragment key={`${c.label}-${i}`}>
            {i > 0 && <span className="sep" aria-hidden="true">/</span>}
            {i === last ? (
              <span className="here">{c.label}</span>
            ) : c.href ? (
              <a href={c.href} className="crumb-link">{c.label}</a>
            ) : (
              <span>{c.label}</span>
            )}
          </React.Fragment>
        ))}
      </nav>

      <span className="grow" />
      {actions}
    </header>
  );
}
