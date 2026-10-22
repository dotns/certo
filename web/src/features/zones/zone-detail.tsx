import type { Scope } from './api'
import { Link, useNavigate } from '@tanstack/react-router'
import { ArrowLeftIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { Alert, AlertDescription, AlertTitle } from '@/shared/components/ui/alert'
import { Button } from '@/shared/components/ui/button'
import { formatUnix } from '@/shared/lib/format'
import { DnssecCard } from './dnssec-card'
import { useDeleteZoneMutation, useRrsetsQuery, useVerifyZoneMutation, useZoneQuery } from './hooks'
import { RrsetTable } from './rrset-table'
import { TransferCard } from './transfer-card'
import { ZoneStatusBadge } from './zone-status'

export function ZoneDetail({ scope, zone }: { scope: Scope, zone: string }) {
  const { t } = useTranslation()
  const query = useZoneQuery(scope, zone)
  const active = query.data?.status === 'active'
  const rrsets = useRrsetsQuery(scope, zone, active)
  const verify = useVerifyZoneMutation(scope)
  const remove = useDeleteZoneMutation(scope)
  const navigate = useNavigate()
  const back = scope === 'admin' ? '/manage' : '/zones'

  useEffect(() => {
    document.title = t('titles.zone', { zone })
  }, [t, zone])

  return (
    <>
      <PageHeader
        title={(
          <span className="flex flex-wrap items-center gap-3">
            <Link to={back} className="text-muted-foreground hover:text-foreground" aria-label={t('zones.detail.back')}><ArrowLeftIcon className="size-5" /></Link>
            <span className="font-mono">{zone}</span>
            {query.data && <ZoneStatusBadge zone={query.data} />}
          </span>
        )}
        description={query.data?.owner ? t('zones.detail.ownedBy', { owner: query.data.owner }) : undefined}
        actions={query.data && !query.data.system && (
          <>
            <Button
              type="button"
              variant="outline"
              disabled={verify.isPending}
              onClick={() => verify.mutate(zone, { onSuccess: z => toast.success(z.status === 'active' ? t('zones.verified.active', { zone }) : t('zones.verified.pending', { zone })) })}
            >
              <RefreshCwIcon data-icon="inline-start" />
              {t('zones.detail.verify')}
            </Button>
            <ConfirmDialog
              trigger={(
                <Button type="button" variant="destructive">
                  <Trash2Icon data-icon="inline-start" />
                  {t('zones.detail.deleteZone')}
                </Button>
              )}
              title={t('zones.delete.title', { zone })}
              description={t('zones.delete.description')}
              confirmLabel={t('zones.delete.confirm')}
              onConfirm={async () => {
                await remove.mutateAsync(zone)
                toast.success(t('zones.delete.done', { zone }))
                navigate({ to: back })
              }}
            />
          </>
        )}
      />
      {query.data && (
        <dl className="flex flex-wrap gap-x-6 gap-y-1 text-sm">
          <Meta label={t('zones.detail.meta.created')} value={formatUnix(query.data.created_at)} />
          <Meta label={t('zones.detail.meta.verified')} value={formatUnix(query.data.verified_at) || t('common.never')} />
          <Meta label={t('zones.detail.meta.lastCheck')} value={formatUnix(query.data.last_check) || t('common.never')} />
          <Meta label={t('zones.detail.meta.nameserver')} value={query.data.ns_target} mono />
          {query.data.exempt && !query.data.system && <Meta label={t('zones.detail.meta.check')} value={t('zones.detail.meta.exempt')} />}
        </dl>
      )}
      <ErrorAlert error={query.error} title={t('zones.detail.loadError')} />
      <ErrorAlert error={verify.error} title={t('zones.detail.verifyTitle')} />
      {query.isPending && <Loading />}
      {query.data && query.data.status === 'pending' && (
        <Alert>
          <AlertTitle>{t('zones.detail.waitingTitle')}</AlertTitle>
          <AlertDescription>
            <p>{t('zones.detail.waitingText', { zone, ns: query.data.ns_target })}</p>
            {query.data.last_check && (
              <p>
                {query.data.last_error
                  ? t('zones.detail.lastCheckError', { time: formatUnix(query.data.last_check), error: query.data.last_error })
                  : t('zones.detail.lastCheck', { time: formatUnix(query.data.last_check) })}
              </p>
            )}
          </AlertDescription>
        </Alert>
      )}
      {query.data && active && (
        <>
          {query.data.failed_checks > 0 && !query.data.exempt && (
            <Alert variant="destructive">
              <AlertTitle>{t('zones.detail.failingTitle')}</AlertTitle>
              <AlertDescription>
                {t('zones.detail.failingText', { count: query.data.failed_checks })}
                {query.data.last_error ? ` ${t('zones.detail.lastError', { error: query.data.last_error })}` : ''}
              </AlertDescription>
            </Alert>
          )}
          <ErrorAlert error={rrsets.error} title={t('zones.detail.recordsError')} />
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

function Meta({ label, value, mono }: { label: string, value: string, mono?: boolean }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className={mono ? 'font-mono text-xs' : ''}>{value}</dd>
    </div>
  )
}
