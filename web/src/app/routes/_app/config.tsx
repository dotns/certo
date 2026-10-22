import type { ClientId, Protocol } from '@/features/config/snippets'
import { createFileRoute } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useProfileQuery } from '@/features/auth/hooks'
import { CLIENTS, clientSnippets, KEY_PLACEHOLDER } from '@/features/config/snippets'
import { useInfoQuery } from '@/features/info/hooks'
import { useKeysQuery } from '@/features/keys/hooks'
import { CodeBlock } from '@/shared/components/code-block'
import { PageHeader } from '@/shared/components/page-header'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Label } from '@/shared/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { Tabs, TabsList, TabsTrigger } from '@/shared/components/ui/tabs'

interface ConfigSearch {
  key?: number
}

export const Route = createFileRoute('/_app/config')({
  validateSearch: (search: Record<string, unknown>): ConfigSearch => {
    const key = Number(search.key)
    return Number.isInteger(key) && key > 0 ? { key } : {}
  },
  component: ConfigPage,
})

const PROTOCOL_NOTE: Record<Protocol, string> = {
  httpreq: 'lego\'s HTTP request provider: the client posts each challenge to this server with your username and API key.',
  acmedns: 'The acme-dns protocol, understood by many clients. Your username and API key are the acme-dns credentials; the CNAME is the same as for httpreq.',
}

function ConfigPage() {
  const info = useInfoQuery()
  const profile = useProfileQuery()
  const keys = useKeysQuery()
  const search = Route.useSearch()
  const navigate = Route.useNavigate()
  const [protocol, setProtocol] = useState<Protocol>('httpreq')
  const [client, setClient] = useState<ClientId>('lego')

  useEffect(() => {
    document.title = 'Clients · certo'
  }, [])

  const selected = keys.data?.find(k => k.id === search.key)
  const apiDomain = info.data?.api_domain ?? window.location.host
  const clients = clientSnippets(protocol, apiDomain, profile.data?.username ?? '<username>', selected?.key ?? KEY_PLACEHOLDER)
  const current = clients.find(c => c.id === client) ?? clients[0]

  return (
    <>
      <PageHeader
        title="Client setup"
        description="Point your ACME client at this server with one of your API keys."
        actions={(
          <div className="flex items-center gap-2">
            <Label htmlFor="snippet-key" className="text-sm text-muted-foreground">API key</Label>
            <Select
              value={selected ? String(selected.id) : ''}
              items={(keys.data ?? []).map(k => ({ value: String(k.id), label: k.name }))}
              onValueChange={v => navigate({ search: v ? { key: Number(v) } : {} })}
            >
              <SelectTrigger id="snippet-key" className="w-48">
                <SelectValue placeholder="Placeholder only" />
              </SelectTrigger>
              <SelectContent>
                {(keys.data ?? []).map(k => <SelectItem key={k.id} value={String(k.id)}>{k.name}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
        )}
      />
      <Tabs value={protocol} onValueChange={v => setProtocol(v as Protocol)}>
        <TabsList aria-label="Protocol">
          <TabsTrigger value="httpreq">httpreq</TabsTrigger>
          <TabsTrigger value="acmedns">acme-dns</TabsTrigger>
        </TabsList>
      </Tabs>
      <p className="text-sm text-muted-foreground">{PROTOCOL_NOTE[protocol]}</p>
      <Tabs value={client} onValueChange={v => setClient(v as ClientId)}>
        <TabsList variant="line" aria-label="Client">
          {CLIENTS.map(c => <TabsTrigger key={c.id} value={c.id}>{c.title}</TabsTrigger>)}
        </TabsList>
      </Tabs>
      {current && (
        <Card>
          <CardContent className="flex flex-col gap-5">
            {current.note && <p className="text-sm text-muted-foreground">{current.note}</p>}
            {current.snippets.map(s => <CodeBlock key={s.title} title={s.title} code={s.code} />)}
            {!selected && current.snippets.length > 0 && (
              <p className="text-xs text-muted-foreground">
                Select an API key above to fill in
                {' '}
                <code>{KEY_PLACEHOLDER}</code>
                .
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </>
  )
}
