import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
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
  const zones = useZonesQuery('user')
  const info = useInfoQuery()
  const navigate = useNavigate()
  const [actionError, setActionError] = useState<unknown>(null)
  const nsTarget = info.data?.ns_target ?? ''

  useEffect(() => {
    document.title = 'Zones · certo'
  }, [])

  return (
    <>
      <PageHeader
        title="Hosted zones"
        description={`Authoritative forward and reverse zones. Delegate a zone to ${nsTarget || 'this server'} and it is served after the next check.`}
        actions={<CreateZoneDialog scope="user" nsTarget={nsTarget} onCreated={name => navigate({ to: '/zones/$zone', params: { zone: name } })} />}
      />
      <ErrorAlert error={zones.error} title="Zones could not be loaded" />
      <ErrorAlert error={actionError} />
      {zones.isPending && <Loading />}
      {zones.data && zones.data.length === 0 && <EmptyState title="No zones yet" description="Add a zone to host its records here." />}
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
