import { MonitorIcon, MoonIcon, SunIcon } from 'lucide-react'
import { Button } from '@/shared/components/ui/button'
import { nextTheme, useTheme } from '@/shared/lib/theme-context'

const ICONS = { light: SunIcon, dark: MoonIcon, system: MonitorIcon }

/** Cycles light → dark → system. */
export function ModeToggle() {
  const { theme, setTheme } = useTheme()
  const Icon = ICONS[theme]
  return (
    <Button type="button" variant="ghost" size="icon-sm" aria-label={`Theme: ${theme}. Switch theme`} onClick={() => setTheme(nextTheme(theme))}>
      <Icon />
    </Button>
  )
}
