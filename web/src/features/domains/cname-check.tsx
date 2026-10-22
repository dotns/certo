import type { CnameCheck } from './api'
import { CheckCircle2Icon, RefreshCwIcon, XCircleIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/shared/components/ui/button'
import { errorMessage } from '@/shared/lib/http'
import { useCheckDomainMutation } from './hooks'

interface CnameCheckButtonProps {
  domain: string
  onResult: (result: CnameCheck) => void
}

/** Runs the server-side CNAME lookup for one domain. */
export function CnameCheckButton({ domain, onResult }: CnameCheckButtonProps) {
  const { t } = useTranslation()
  const check = useCheckDomainMutation()
  return (
    <Button
      type="button"
      variant="outline"
      size="xs"
      disabled={check.isPending}
      title={check.error ? errorMessage(check.error) : undefined}
      onClick={() => check.mutate(domain, { onSuccess: onResult })}
    >
      <RefreshCwIcon data-icon="inline-start" className={check.isPending ? 'animate-spin' : ''} />
      {check.error ? t('domains.couldNotCheck') : t('domains.check')}
    </Button>
  )
}

/** The outcome of a CNAME check as a short status line. */
export function CnameResult({ result }: { result: CnameCheck }) {
  const { t } = useTranslation()
  if (result.ok) {
    return (
      <span className="inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
        <CheckCircle2Icon className="size-3.5" />
        {t('domains.inPlace')}
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1 text-xs text-destructive">
      <XCircleIcon className="size-3.5" />
      {result.observed ? t('domains.pointsAt', { target: result.observed }) : t('domains.noCname')}
    </span>
  )
}
