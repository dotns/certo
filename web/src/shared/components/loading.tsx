import { useTranslation } from 'react-i18next'
import { Spinner } from '@/shared/components/ui/spinner'

export function Loading({ label }: { label?: string }) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground" role="status">
      <Spinner aria-hidden="true" />
      {label ?? t('common.loading')}
    </div>
  )
}
