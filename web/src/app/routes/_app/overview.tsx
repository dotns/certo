import { createFileRoute, Link } from '@tanstack/react-router'
import { useEffect } from 'react'
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
  { title: 'Add a challenge domain', text: 'Register the domain you request certificates for; certo assigns it a unique CNAME target.', to: '/domains', label: 'Domains' },
  { title: 'Point the CNAME at it', text: 'At your DNS provider, create _acme-challenge.<domain> CNAME <target>. The Domains page checks it for you.', to: '/domains', label: 'Check CNAME' },
  { title: 'Configure your ACME client', text: 'Use an API key with lego, Traefik, Caddy, acme.sh or cert-manager; the Clients page has ready-made snippets.', to: '/config', label: 'Clients' },
] as const

function OverviewPage() {
  const info = useInfoQuery()
  const profile = useProfileQuery()
  const domains = useDomainsQuery()
  const records = useRecordsQuery()
  const zones = useZonesQuery('user')
  const keys = useKeysQuery()

  useEffect(() => {
    document.title = 'Overview · certo'
  }, [])

  const count = (n: number | undefined) => (n === undefined ? '…' : n)
  const active = zones.data?.filter(z => z.status === 'active').length
  const pending = zones.data?.filter(z => z.status !== 'active').length

  return (
    <>
      <PageHeader title={profile.data ? `Hello, ${profile.data.username}` : 'Overview'} description="Your challenge domains, hosted zones and keys at a glance." />
      <ErrorAlert error={info.error ?? domains.error ?? zones.error ?? keys.error} />
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard label="Challenge domains" value={count(domains.data?.length)} to="/domains" />
        <StatCard label="Published tokens" value={count(records.data?.length)} hint="TXT records live right now" to="/domains" />
        <StatCard label="Hosted zones" value={count(zones.data?.length)} hint={zones.data ? `${active} active · ${pending} pending` : undefined} to="/zones" />
        <StatCard label="API keys" value={count(keys.data?.length)} to="/keys" />
      </div>
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Server</CardTitle>
            <CardDescription>What this certo instance serves.</CardDescription>
          </CardHeader>
          <CardContent>
            {info.data && (
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <Row label="Base zone" value={info.data.base_zone} />
                <Row label="Challenge domain" value={info.data.base_domain} hint="challenge records live at <subdomain>.this" />
                <Row label="Nameserver" value={info.data.ns_target} hint="delegate hosted zones here" />
                <Row label="API host" value={info.data.api_domain} />
                <Row label="Version" value={info.data.version} plain />
                <dt className="text-muted-foreground">Registration</dt>
                <dd>{info.data.registration ? <Badge variant="secondary">Open</Badge> : <Badge variant="outline">Closed</Badge>}</dd>
              </dl>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle>Quick start</CardTitle>
            <CardDescription>Three steps to DNS-01 certificates without exposing your zone.</CardDescription>
          </CardHeader>
          <CardContent>
            <ol className="flex flex-col gap-4">
              {STEPS.map((s, i) => (
                <li key={s.title} className="flex gap-3">
                  <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-semibold">{i + 1}</span>
                  <div className="flex flex-1 flex-col gap-1">
                    <span className="text-sm font-medium">{s.title}</span>
                    <span className="text-sm text-muted-foreground">{s.text}</span>
                    <Link to={s.to} className="self-start"><Button type="button" variant="outline" size="xs">{s.label}</Button></Link>
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
