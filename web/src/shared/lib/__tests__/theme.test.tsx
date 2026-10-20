import { act, render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { ThemeProvider } from '../theme'
import { nextTheme, useTheme } from '../theme-context'

function Probe() {
  const { theme, setTheme } = useTheme()
  return (
    <button type="button" onClick={() => setTheme(nextTheme(theme))}>{theme}</button>
  )
}

describe('theme', () => {
  it('cycles light → dark → system', () => {
    expect(nextTheme('light')).toBe('dark')
    expect(nextTheme('dark')).toBe('system')
    expect(nextTheme('system')).toBe('light')
  })

  it('applies the dark class and persists the choice', () => {
    localStorage.setItem('certo-theme', 'light')
    render(<ThemeProvider><Probe /></ThemeProvider>)
    const button = screen.getByRole('button')
    expect(button).toHaveTextContent('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    act(() => button.click())
    expect(button).toHaveTextContent('dark')
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(localStorage.getItem('certo-theme')).toBe('dark')
  })
})
