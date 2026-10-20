import { Spinner } from '@/shared/components/ui/spinner'

export function Loading({ label = 'Loading' }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground" role="status">
      <Spinner aria-hidden="true" />
      {label}
    </div>
  )
}
