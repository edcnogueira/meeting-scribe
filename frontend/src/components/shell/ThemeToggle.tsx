'use client';

import React from 'react';
import { useTheme } from '@/components/theme/ThemeProvider';
import { SunIcon, MoonIcon } from './icons';

/**
 * Sidebar footer theme toggle (task R1). Renders the sun icon in dark mode and
 * the moon icon in light mode, matching the design's `.js-theme` button.
 */
export function ThemeToggle({ className }: { className?: string }) {
  const { theme, toggleTheme } = useTheme();
  const isDark = theme === 'dark';
  return (
    <button
      type="button"
      className={`icon-btn ${className ?? ''}`}
      onClick={toggleTheme}
      data-tip={isDark ? 'Tema claro' : 'Tema escuro'}
      aria-label={isDark ? 'Switch to light theme' : 'Switch to dark theme'}
    >
      {isDark ? <SunIcon /> : <MoonIcon />}
    </button>
  );
}
