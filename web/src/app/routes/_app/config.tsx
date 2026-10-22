import type { ClientId, Protocol } from '@/features/config/snippets'
import { createFileRoute } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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

function ConfigPage() {
  const { t } = useTranslation()
  const info = useInfoQuery()
  const profile = useProfileQuery()
  const keys = useKeysQuery()
  const search = Route.useSearch()
  const navigate = Route.useNavigate()
  const [protocol, setProtocol] = useState<Protocol>('httpreq')
  const [client, setClient] = useState<ClientId>('lego')

  useEffect(() => {
    document.title = t('titles.clients')
  }, [t])

  const selected = keys.data?.find(k => k.id === search.key)
  const apiDomain = info.data?.api_domain ?? window.location.host
  const clients = clientSnippets(protocol, apiDomain, profile.data?.username ?? '<username>', selected?.key ?? KEY_PLACEHOLDER)
  const current = clients.find(c => c.id === client) ?? clients[0]

  return (
    <>
      <PageHeader
        title={t('clients.title')}
        description={t('clients.description')}
        actions={(
          <div className="flex items-center gap-2">
            <Label htmlFor="snippet-key" className="text-sm text-muted-foreground">{t('clients.keyLabel')}</Label>
            <Select
              value={selected ? String(selected.id) : ''}
              items={(keys.data ?? []).map(k => ({ value: String(k.id), label: k.name }))}
              onValueChange={v => navigate({ search: v ? { key: Number(v) } : {} })}
            >
              <SelectTrigger id="snippet-key" className="w-48">
                <SelectValue placeholder={t('clients.keyPlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {(keys.data ?? []).map(k => <SelectItem key={k.id} value={String(k.id)}>{k.name}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
        )}
      />
      <Tabs value={protocol} onValueChange={v => setProtocol(v as Protocol)}>
        <TabsList aria-label={t('clients.protocol')}>
          <TabsTrigger value="httpreq">httpreq</TabsTrigger>
          <TabsTrigger value="acmedns">acme-dns</TabsTrigger>
        </TabsList>
      </Tabs>
      <p className="text-sm text-muted-foreground">{t(`clients.protocolNote.${protocol}`)}</p>
      <Tabs value={client} onValueChange={v => setClient(v as ClientId)}>
        <TabsList variant="line" aria-label={t('clients.client')}>
          {CLIENTS.map(c => <TabsTrigger key={c.id} value={c.id}>{c.title}</TabsTrigger>)}
        </TabsList>
      </Tabs>
      {current && (
        <Card>
          <CardContent className="flex flex-col gap-5">
            {current.note && <p className="text-sm text-muted-foreground">{t(current.note, { client: current.title })}</p>}
            {current.snippets.map(s => <CodeBlock key={s.title} title={t(s.title)} code={s.code} />)}
            {!selected && current.snippets.length > 0 && (
              <p className="text-xs text-muted-foreground">{t('clients.selectKeyHint', { placeholder: KEY_PLACEHOLDER })}</p>
            )}
          </CardContent>
        </Card>
      )}
    </>
  )
}
