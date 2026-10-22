import { LanguagesIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { LANGUAGES } from '@/app/i18n'
import { Button } from '@/shared/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/shared/components/ui/dropdown-menu'

/** Switches the interface language; the choice is persisted by the detector. */
export function LanguageToggle() {
  const { t, i18n } = useTranslation()
  // Load the bundle first so the switch itself never suspends the tree.
  const change = (code: string) => i18n.loadLanguages(code).then(() => i18n.changeLanguage(code))
  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" aria-label={t('nav.language')} />}>
        <LanguagesIcon />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        <DropdownMenuRadioGroup value={i18n.resolvedLanguage ?? i18n.language} onValueChange={v => change(String(v))}>
          {LANGUAGES.map(l => <DropdownMenuRadioItem key={l.code} value={l.code} closeOnClick>{l.label}</DropdownMenuRadioItem>)}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
