import { AlertCircleIcon } from 'lucide-react'
import { Alert, AlertDescription, AlertTitle } from '@/shared/components/ui/alert'
import { errorMessage } from '@/shared/lib/http'

interface ErrorAlertProps {
  error: unknown
  title?: string
}

/** Shows an API or network error; renders nothing when `error` is falsy. */
export function ErrorAlert({ error, title }: ErrorAlertProps) {
  if (!error)
    return null
  return (
    <Alert variant="destructive">
      <AlertCircleIcon />
      <AlertTitle>{title ?? 'Something went wrong'}</AlertTitle>
      <AlertDescription>{errorMessage(error)}</AlertDescription>
    </Alert>
  )
}
