import { Link } from '@tanstack/react-router'
import { useInfoQuery } from '@/features/info/hooks'
import { useZonesQuery } from '@/features/zones/hooks'
import { CopyButton } from '@/shared/components/copy-button'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { Badge } from '@/shared/components/ui/badge'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { useAdminDomainsQuery, useAdminRecordsQuery, useAdminUsersQuery } from './hooks'

export function AdminServer() {
  const info = useInfoQuery()
  const users = useAdminUsersQuery()
  const domains = useAdminDomainsQuery()
  const records = useAdminRecordsQuery()
  const zones = useZonesQuery('admin')
  const count = (n: number | undefined) => (n === undefined ? '…' : String(n))

  return (
    <div className="grid gap-6 pt-2 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle>Configuration</CardTitle>
          <CardDescription>Values from the server config, as reported by the public info endpoint.</CardDescription>
        </CardHeader>
        <CardContent>
          <ErrorAlert error={info.error} />
          {info.isPending && <Loading />}
          {info.data && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt className="text-muted-foreground">Base zone</dt>
              <dd className="flex flex-wrap items-center gap-2">
                <span className="font-mono text-xs">{info.data.base_zone}</span>
                <Link to="/manage/zones/$zone" params={{ zone: info.data.base_zone }}><Button type="button" variant="outline" size="xs">Edit records</Button></Link>
              </dd>
              <Entry label="Challenge domain" value={info.data.base_domain} />
              <Entry label="Nameserver" value={info.data.ns_target} />
              <Entry label="API host" value={info.data.api_domain} />
              <dt className="text-muted-foreground">Registration</dt>
              <dd>{info.data.registration ? <Badge variant="secondary">Open</Badge> : <Badge variant="outline">Closed</Badge>}</dd>
              <dt className="text-muted-foreground">Version</dt>
              <dd>{info.data.version}</dd>
              <dt className="text-muted-foreground">Capabilities</dt>
              <dd className="flex flex-wrap gap-1">{info.data.capabilities.map(c => <Badge key={c} variant="outline">{c}</Badge>)}</dd>
            </dl>
          )}
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle>Totals</CardTitle>
          <CardDescription>Across all accounts.</CardDescription>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">Users</dt>
            <dd className="tabular-nums">{count(users.data?.length)}</dd>
            <dt className="text-muted-foreground">Challenge domains</dt>
            <dd className="tabular-nums">{count(domains.data?.length)}</dd>
            <dt className="text-muted-foreground">Published tokens</dt>
            <dd className="tabular-nums">{count(records.data?.length)}</dd>
            <dt className="text-muted-foreground">Hosted zones</dt>
            <dd className="tabular-nums">
              {count(zones.data?.length)}
              {zones.data && (
                <span className="text-muted-foreground">
                  {` (${zones.data.filter(z => z.status === 'active').length} active, ${zones.data.filter(z => z.status !== 'active').length} pending)`}
                </span>
              )}
            </dd>
          </dl>
        </CardContent>
      </Card>
    </div>
  )
}

function Entry({ label, value }: { label: string, value: string }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="flex items-center gap-1 font-mono text-xs">
        {value}
        <CopyButton text={value} label="" />
      </dd>
    </>
  )
}
