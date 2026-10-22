import type { CnameCheck } from './api'
import { CheckCircle2Icon, RefreshCwIcon, XCircleIcon } from 'lucide-react'
import { Button } from '@/shared/components/ui/button'
import { errorMessage } from '@/shared/lib/http'
import { useCheckDomainMutation } from './hooks'

interface CnameCheckButtonProps {
  domain: string
  onResult: (result: CnameCheck) => void
}

/** Runs the server-side CNAME lookup for one domain. */
export function CnameCheckButton({ domain, onResult }: CnameCheckButtonProps) {
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
      {check.error ? 'Could not check' : 'Check CNAME'}
    </Button>
  )
}

/** The outcome of a CNAME check as a short status line. */
export function CnameResult({ result }: { result: CnameCheck }) {
  if (result.ok) {
    return (
      <span className="inline-flex items-center gap-1 text-xs text-emerald-600 dark:text-emerald-400">
        <CheckCircle2Icon className="size-3.5" />
        CNAME in place
      </span>
    )
  }
  return (
    <span className="inline-flex items-center gap-1 text-xs text-destructive">
      <XCircleIcon className="size-3.5" />
      {result.observed ? `Points at ${result.observed}` : 'No CNAME found'}
    </span>
  )
}
