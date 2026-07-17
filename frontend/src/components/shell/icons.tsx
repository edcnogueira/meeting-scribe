'use client';

import React from 'react';

/**
 * Shared shell icons — 1:1 with the design export (js/app.js ICONS).
 * Stroke icons using currentColor so they inherit token-driven colors.
 */

export const ChevIcon = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
    <path d="M3.5 2l3 3-3 3" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const FolderIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h2.9l1.5 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4v-7.6z" stroke="currentColor" strokeWidth="1.2" />
  </svg>
);

export const DocIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <rect x="3" y="2" width="10" height="12" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
    <path d="M5.6 5.5h4.8M5.6 8h4.8M5.6 10.5h2.8" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
  </svg>
);

export const GearIcon = () => (
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <circle cx="8" cy="8" r="2.2" stroke="currentColor" strokeWidth="1.2" />
    <path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M12.4 3.6l-1.2 1.2M4.8 11.2l-1.2 1.2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </svg>
);

export const PanelIcon = () => (
  <svg width="15" height="15" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <rect x="1.8" y="2.5" width="12.4" height="11" rx="1.6" stroke="currentColor" strokeWidth="1.2" />
    <path d="M6 2.5v11" stroke="currentColor" strokeWidth="1.2" />
  </svg>
);

export const RefreshIcon = () => (
  <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M13.2 6.6A5.4 5.4 0 003.4 4.9M2.8 9.4a5.4 5.4 0 009.8 1.7" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
    <path d="M3.2 2.4v2.8H6M12.8 13.6v-2.8H10" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);

export const MoonIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <path d="M13.4 9.6A5.6 5.6 0 016.4 2.6a5.6 5.6 0 107 7z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
  </svg>
);

export const SunIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <circle cx="8" cy="8" r="3" stroke="currentColor" strokeWidth="1.2" />
    <path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M12.6 3.4l-1.1 1.1M4.5 11.5l-1.1 1.1" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </svg>
);

export const MicIcon = () => (
  <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
    <rect x="5.6" y="1.8" width="4.8" height="8" rx="2.4" stroke="currentColor" strokeWidth="1.2" />
    <path d="M3.2 7.6a4.8 4.8 0 009.6 0M8 12.4v2" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
  </svg>
);
