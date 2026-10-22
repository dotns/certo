import { CheckIcon, CopyIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/components/ui/button'

interface CopyButtonProps {
  text: string
  /** Visible label; empty for an icon-only button. */
  label?: string
  size?: 'xs' | 'sm' | 'default'
}

/** Copies `text` to the clipboard and confirms briefly. */
export function CopyButton({ text, label, size = 'xs' }: CopyButtonProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const visible = label ?? t('common.copy')

  useEffect(() => {
    if (!copied)
      return
    const t = setTimeout(setCopied, 1500, false)
    return () => clearTimeout(t)
  }, [copied])

  return (
    <Button
      type="button"
      variant="ghost"
      size={size}
      aria-label={`${t('common.copy')}: ${text}`}
      onClick={() => {
        navigator.clipboard.writeText(text).then(() => setCopied(true), () => {})
      }}
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
      {copied ? t('common.copied') : visible}
    </Button>
  )
}
