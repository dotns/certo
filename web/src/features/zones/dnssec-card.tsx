import type { Scope } from './api'
import { useState } from 'react'
import { CodeBlock } from '@/shared/components/code-block'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { Badge } from '@/shared/components/ui/badge'
import { Button } from '@/shared/components/ui/button'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Label } from '@/shared/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { useDnssecQuery, useSetDnssecMutation } from './hooks'
import { dsLines, NSEC_MODES } from './rrsets'

export function DnssecCard({ scope, zone }: { scope: Scope, zone: string }) {
  const dnssec = useDnssecQuery(scope, zone, true)
  const set = useSetDnssecMutation(scope, zone)
  const [mode, setMode] = useState<string | null>(null)
  const nsecMode = mode ?? dnssec.data?.nsec_mode ?? 'nsec'

  return (
    <Card>
      <CardHeader>
        <CardTitle>DNSSEC</CardTitle>
        <CardDescription>Signing with an ECDSA P-256 key; publish the DS record at the parent.</CardDescription>
        <CardAction>
          {dnssec.data && (dnssec.data.enabled
            ? (
                <Badge>
                  Signed ·
                  {dnssec.data.nsec_mode.toUpperCase()}
                </Badge>
              )
            : <Badge variant="outline">Off</Badge>)}
        </CardAction>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <ErrorAlert error={dnssec.error ?? set.error} />
        {dnssec.isPending && <Loading />}
        {dnssec.data && (
          <div className="flex flex-wrap items-end gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="nsec-mode">Denial of existence</Label>
              <Select value={nsecMode} onValueChange={v => setMode(String(v))}>
                <SelectTrigger id="nsec-mode" className="w-32">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {NSEC_MODES.map(m => <SelectItem key={m} value={m}>{m.toUpperCase()}</SelectItem>)}
                </SelectContent>
              </Select>
            </div>
            {dnssec.data.enabled
              ? (
                  <>
                    <Button type="button" variant="outline" disabled={set.isPending || nsecMode === dnssec.data.nsec_mode} onClick={() => set.mutate({ enabled: true, nsec_mode: nsecMode })}>
                      Apply
                    </Button>
                    <ConfirmDialog
                      trigger={<Button variant="destructive" disabled={set.isPending}>Disable</Button>}
                      title="Disable DNSSEC?"
                      description="Remove the DS record at the parent first, or resolvers will reject the zone."
                      confirmLabel="Disable"
                      onConfirm={() => set.mutateAsync({ enabled: false, nsec_mode: nsecMode })}
                    />
                  </>
                )
              : (
                  <Button type="button" disabled={set.isPending} onClick={() => set.mutate({ enabled: true, nsec_mode: nsecMode })}>
                    Enable DNSSEC
                  </Button>
                )}
          </div>
        )}
        {dnssec.data?.ds && dnssec.data.ds.length > 0 && <CodeBlock title="DS records for the parent zone" code={dsLines(zone, dnssec.data.ds)} wrap />}
        {dnssec.data?.dnskey && <CodeBlock title="DNSKEY" code={`${zone}. IN DNSKEY ${dnssec.data.dnskey}`} wrap />}
      </CardContent>
    </Card>
  )
}
