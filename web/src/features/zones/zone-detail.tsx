import type { Scope } from './api'
import { Link } from '@tanstack/react-router'
import { ArrowLeftIcon, RefreshCwIcon } from 'lucide-react'
import { useEffect } from 'react'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { Alert, AlertDescription, AlertTitle } from '@/shared/components/ui/alert'
import { Button } from '@/shared/components/ui/button'
import { formatUnix } from '@/shared/lib/format'
import { DnssecCard } from './dnssec-card'
import { useRrsetsQuery, useVerifyZoneMutation, useZoneQuery } from './hooks'
import { RrsetTable } from './rrset-table'
import { TransferCard } from './transfer-card'
import { ZoneStatusBadge } from './zone-status'

export function ZoneDetail({ scope, zone }: { scope: Scope, zone: string }) {
  const query = useZoneQuery(scope, zone)
  const active = query.data?.status === 'active'
  const rrsets = useRrsetsQuery(scope, zone, active)
  const verify = useVerifyZoneMutation(scope)
  const back = scope === 'admin' ? '/manage' : '/zones'

  useEffect(() => {
    document.title = `${zone} · certo`
  }, [zone])

  return (
    <>
      <PageHeader
        title={(
          <span className="flex flex-wrap items-center gap-3">
            <Link to={back} className="text-muted-foreground hover:text-foreground" aria-label="Back to zones"><ArrowLeftIcon className="size-5" /></Link>
            <span className="font-mono">{zone}</span>
            {query.data && <ZoneStatusBadge zone={query.data} />}
          </span>
        )}
        description={query.data?.owner ? `Owned by ${query.data.owner}` : undefined}
        actions={query.data && !query.data.system && (
          <Button type="button" variant="outline" disabled={verify.isPending} onClick={() => verify.mutate(zone)}>
            <RefreshCwIcon data-icon="inline-start" />
            Verify delegation
          </Button>
        )}
      />
      <ErrorAlert error={query.error} title="Zone could not be loaded" />
      <ErrorAlert error={verify.error} title="Verification" />
      {query.isPending && <Loading />}
      {query.data && query.data.status === 'pending' && (
        <Alert>
          <AlertTitle>Waiting for delegation</AlertTitle>
          <AlertDescription>
            <p>
              At the parent (registrar or address block owner), add an NS record for
              {' '}
              <code>{zone}</code>
              {' '}
              pointing at
              {' '}
              <code>{query.data.ns_target}</code>
              . The zone is checked hourly and served once the delegation is found.
            </p>
            {query.data.last_check && (
              <p>
                Last check
                {' '}
                {formatUnix(query.data.last_check)}
                {query.data.last_error ? `: ${query.data.last_error}` : ''}
              </p>
            )}
          </AlertDescription>
        </Alert>
      )}
      {query.data && active && (
        <>
          {query.data.failed_checks > 0 && !query.data.exempt && (
            <Alert variant="destructive">
              <AlertTitle>Delegation check failing</AlertTitle>
              <AlertDescription>
                {query.data.failed_checks}
                {' '}
                consecutive failed checks; the zone stops being served after 7.
                {query.data.last_error ? ` Last error: ${query.data.last_error}` : ''}
              </AlertDescription>
            </Alert>
          )}
          <ErrorAlert error={rrsets.error} title="Records could not be loaded" />
          {rrsets.isPending && <Loading />}
          {rrsets.data && <RrsetTable scope={scope} zone={zone} rrsets={rrsets.data} />}
          <div className="grid gap-6 lg:grid-cols-2">
            <DnssecCard scope={scope} zone={zone} />
            <TransferCard scope={scope} zone={zone} nsTarget={query.data.ns_target} />
          </div>
        </>
      )}
    </>
  )
}
