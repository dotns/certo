import { createContext, use } from 'react'

export type Theme = 'light' | 'dark' | 'system'

export const THEME_STORAGE_KEY = 'certo-theme'
export const THEMES: Theme[] = ['light', 'dark', 'system']

export interface ThemeContextValue {
  theme: Theme
  setTheme: (theme: Theme) => void
}

export const ThemeContext = createContext<ThemeContextValue | null>(null)

export function useTheme(): ThemeContextValue {
  const ctx = use(ThemeContext)
  if (!ctx)
    throw new Error('useTheme must be used within ThemeProvider')
  return ctx
}

/** The theme after `current` when cycling light → dark → system. */
export function nextTheme(current: Theme): Theme {
  return THEMES[(THEMES.indexOf(current) + 1) % THEMES.length] ?? 'system'
}
