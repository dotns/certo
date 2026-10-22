import { createFileRoute, Link } from '@tanstack/react-router'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useProfileQuery } from '@/features/auth/hooks'
import { useDomainsQuery, useRecordsQuery } from '@/features/domains/hooks'
import { useInfoQuery } from '@/features/info/hooks'
import { useKeysQuery } from '@/features/keys/hooks'
import { useZonesQuery } from '@/features/zones/hooks'
import { CopyButton } from '@/shared/components/copy-button'
import { ErrorAlert } from '@/shared/components/error-alert'
import { PageHeader } from '@/shared/components/page-header'
import { StatCard } from '@/shared/components/stat-card'
import { Badge } from '@/shared/components/ui/badge'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'

export const Route = createFileRoute('/_app/overview')({
  component: OverviewPage,
})

const STEPS = [
  { key: 'step1', to: '/domains' },
  { key: 'step2', to: '/domains' },
  { key: 'step3', to: '/config' },
] as const

function OverviewPage() {
  const { t } = useTranslation()
  const info = useInfoQuery()
  const profile = useProfileQuery()
  const domains = useDomainsQuery()
  const records = useRecordsQuery()
  const zones = useZonesQuery('user')
  const keys = useKeysQuery()

  useEffect(() => {
    document.title = t('titles.overview')
  }, [t])

  const count = (n: number | undefined) => (n === undefined ? '…' : n)
  const active = zones.data?.filter(z => z.status === 'active').length ?? 0
  const pending = zones.data?.filter(z => z.status !== 'active').length ?? 0

  return (
    <>
      <PageHeader title={profile.data ? t('overview.hello', { name: profile.data.username }) : t('overview.title')} description={t('overview.description')} />
      <ErrorAlert error={info.error ?? domains.error ?? zones.error ?? keys.error} />
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard label={t('overview.stats.domains')} value={count(domains.data?.length)} to="/domains" />
        <StatCard label={t('overview.stats.tokens')} value={count(records.data?.length)} hint={t('overview.stats.tokensHint')} to="/domains" />
        <StatCard label={t('overview.stats.zones')} value={count(zones.data?.length)} hint={zones.data ? t('overview.stats.zonesHint', { active, pending }) : undefined} to="/zones" />
        <StatCard label={t('overview.stats.keys')} value={count(keys.data?.length)} to="/keys" />
      </div>
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>{t('overview.server.title')}</CardTitle>
            <CardDescription>{t('overview.server.description')}</CardDescription>
          </CardHeader>
          <CardContent>
            {info.data && (
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <Row label={t('overview.server.baseZone')} value={info.data.base_zone} />
                <Row label={t('overview.server.challengeDomain')} value={info.data.base_domain} hint={t('overview.server.challengeHint')} />
                <Row label={t('overview.server.nameserver')} value={info.data.ns_target} hint={t('overview.server.nameserverHint')} />
                <Row label={t('overview.server.apiHost')} value={info.data.api_domain} />
                <Row label={t('overview.server.version')} value={info.data.version} plain />
                <dt className="text-muted-foreground">{t('overview.server.registration')}</dt>
                <dd>{info.data.registration ? <Badge variant="secondary">{t('overview.server.open')}</Badge> : <Badge variant="outline">{t('overview.server.closed')}</Badge>}</dd>
              </dl>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>{t('overview.quickStart.title')}</CardTitle>
            <CardDescription>{t('overview.quickStart.description')}</CardDescription>
          </CardHeader>
          <CardContent>
            <ol className="flex flex-col gap-4">
              {STEPS.map((s, i) => (
                <li key={s.key} className="flex gap-3">
                  <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold">{i + 1}</span>
                  <div className="flex flex-1 flex-col gap-1">
                    <span className="text-sm font-medium">{t(`overview.quickStart.${s.key}.title`)}</span>
                    <span className="text-sm text-muted-foreground">{t(`overview.quickStart.${s.key}.text`)}</span>
                    <Link to={s.to} className="self-start"><Button type="button" variant="outline" size="xs">{t(`overview.quickStart.${s.key}.label`)}</Button></Link>
                  </div>
                </li>
              ))}
            </ol>
          </CardContent>
        </Card>
      </div>
    </>
  )
}

function Row({ label, value, hint, plain }: { label: string, value: string, hint?: string, plain?: boolean }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="flex flex-wrap items-center gap-x-2">
        {plain ? <span>{value}</span> : <span className="font-mono text-xs">{value}</span>}
        {!plain && <CopyButton text={value} label="" />}
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </dd>
    </>
  )
}
