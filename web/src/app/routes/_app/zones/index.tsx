import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useInfoQuery } from '@/features/info/hooks'
import { CreateZoneDialog } from '@/features/zones/create-zone-dialog'
import { useZonesQuery } from '@/features/zones/hooks'
import { ZoneTable } from '@/features/zones/zone-table'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { Card, CardContent } from '@/shared/components/ui/card'

export const Route = createFileRoute('/_app/zones/')({
  component: ZonesPage,
})

function ZonesPage() {
  const { t } = useTranslation()
  const zones = useZonesQuery('user')
  const info = useInfoQuery()
  const navigate = useNavigate()
  const [actionError, setActionError] = useState<unknown>(null)
  const nsTarget = info.data?.ns_target ?? ''

  useEffect(() => {
    document.title = t('titles.zones')
  }, [t])

  return (
    <>
      <PageHeader
        title={t('zones.title')}
        description={t('zones.description', { ns: nsTarget || t('zones.thisServer') })}
        actions={<CreateZoneDialog scope="user" nsTarget={nsTarget} onCreated={name => navigate({ to: '/zones/$zone', params: { zone: name } })} />}
      />
      <ErrorAlert error={zones.error} title={t('zones.loadError')} />
      <ErrorAlert error={actionError} />
      {zones.isPending && <Loading />}
      {zones.data && zones.data.length === 0 && <EmptyState title={t('zones.empty.title')} description={t('zones.empty.description')} />}
      {zones.data && zones.data.length > 0 && (
        <Card>
          <CardContent>
            <ZoneTable scope="user" zones={zones.data} onError={setActionError} />
          </CardContent>
        </Card>
      )}
    </>
  )
}
