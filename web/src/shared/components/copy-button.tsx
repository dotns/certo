import { CheckIcon, CopyIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from '@/shared/components/ui/button'

interface CopyButtonProps {
  text: string
  label?: string
  size?: 'xs' | 'sm' | 'default'
}

/** Copies `text` to the clipboard and confirms briefly. */
export function CopyButton({ text, label = 'Copy', size = 'xs' }: CopyButtonProps) {
  const [copied, setCopied] = useState(false)

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
      aria-label={`${label}: ${text}`}
      onClick={() => {
        navigator.clipboard.writeText(text).then(() => setCopied(true), () => {})
      }}
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
      {copied ? 'Copied' : label}
    </Button>
  )
}
