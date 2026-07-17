'use client';

import React, { createContext, useContext, useCallback, useEffect, useState } from 'react';

export type Theme = 'light' | 'dark';

const THEME_KEY = 'ms-theme';

interface ThemeContextValue {
  theme: Theme;
  setTheme: (t: Theme) => void;
  toggleTheme: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

/**
 * Attribute-based theming for the 2026-07 redesign (task R1).
 *
 * The active theme is expressed as `data-theme="light|dark"` on <html> and
 * <body> (the redesign's design tokens are scoped to that attribute), and the
 * choice is persisted in localStorage. The `.dark` class is mirrored on <html>
 * so the pre-existing shadcn/Tailwind color variables track the same choice
 * while R2–R4 migrate individual screens.
 *
 * A blocking inline script (see ThemeNoFlashScript) applies the persisted theme
 * before hydration to avoid a wrong-theme flash.
 */
export function applyTheme(t: Theme) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.setAttribute('data-theme', t);
  root.classList.toggle('dark', t === 'dark');
  if (document.body) document.body.setAttribute('data-theme', t);
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {
    /* storage unavailable — ignore */
  }
}

function readStoredTheme(): Theme {
  if (typeof window === 'undefined') return 'light';
  try {
    const stored = window.localStorage.getItem(THEME_KEY);
    if (stored === 'dark' || stored === 'light') return stored;
  } catch {
    /* ignore */
  }
  return 'light';
}

/**
 * Inline script rendered in <head> so the persisted theme is applied before
 * first paint. Kept dependency-free and defensive (wrapped in try/catch).
 */
export function ThemeNoFlashScript() {
  const js = `(function(){try{var t=localStorage.getItem('${THEME_KEY}');if(t!=='dark'&&t!=='light'){t='light';}var r=document.documentElement;r.setAttribute('data-theme',t);if(t==='dark'){r.classList.add('dark');}}catch(e){}})();`;
  return <script dangerouslySetInnerHTML={{ __html: js }} />;
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setThemeState] = useState<Theme>('light');

  // Sync React state with whatever the no-flash script already applied.
  useEffect(() => {
    const initial = readStoredTheme();
    setThemeState(initial);
    applyTheme(initial);
  }, []);

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t);
    applyTheme(t);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((prev) => {
      const next: Theme = prev === 'dark' ? 'light' : 'dark';
      applyTheme(next);
      return next;
    });
  }, []);

  return (
    <ThemeContext.Provider value={{ theme, setTheme, toggleTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return ctx;
}
