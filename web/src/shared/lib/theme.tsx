import type { ReactNode } from 'react'
import type { Theme, ThemeContextValue } from './theme-context'
import { useEffect, useMemo, useState } from 'react'
import { THEME_STORAGE_KEY, ThemeContext, THEMES } from './theme-context'

function readTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY)
    return THEMES.includes(stored as Theme) ? (stored as Theme) : 'system'
  }
  catch {
    return 'system'
  }
}

function prefersDark(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(readTheme)

  useEffect(() => {
    const root = document.documentElement
    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && prefersDark())
      root.classList.toggle('dark', dark)
    }
    apply()
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [theme])

  const value = useMemo<ThemeContextValue>(() => ({
    theme,
    setTheme: (next) => {
      try {
        localStorage.setItem(THEME_STORAGE_KEY, next)
      }
      catch {
        // storage unavailable: theme lives for this page only
      }
      setTheme(next)
    },
  }), [theme])

  return <ThemeContext value={value}>{children}</ThemeContext>
}
