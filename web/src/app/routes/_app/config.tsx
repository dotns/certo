import type { Protocol } from '@/features/config/snippets'
import { createFileRoute } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useProfileQuery } from '@/features/auth/hooks'
import { clientSnippets } from '@/features/config/snippets'
import { useInfoQuery } from '@/features/info/hooks'
import { CodeBlock } from '@/shared/components/code-block'
import { PageHeader } from '@/shared/components/page-header'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/components/ui/tabs'

export const Route = createFileRoute('/_app/config')({
  component: ConfigPage,
})

function ConfigPage() {
  const info = useInfoQuery()
  const profile = useProfileQuery()
  const [protocol, setProtocol] = useState<Protocol>('httpreq')

  useEffect(() => {
    document.title = 'Clients · certo'
  }, [])

  const set = clientSnippets(protocol, info.data?.api_domain ?? window.location.host, profile.data?.username ?? '<username>')

  return (
    <>
      <PageHeader title="Client setup" description="Point your ACME client at this server with one of your API keys." />
      <Tabs value={protocol} onValueChange={v => setProtocol(v as Protocol)}>
        <TabsList>
          <TabsTrigger value="httpreq">httpreq</TabsTrigger>
          <TabsTrigger value="acmedns">acme-dns</TabsTrigger>
        </TabsList>
        {(['httpreq', 'acmedns'] as const).map(p => (
          <TabsContent key={p} value={p}>
            <Card>
              <CardContent className="flex flex-col gap-5">
                {set.note && <p className="text-sm text-muted-foreground">{set.note}</p>}
                {set.snippets.map(s => <CodeBlock key={s.title} title={s.title} code={s.code} />)}
              </CardContent>
            </Card>
          </TabsContent>
        ))}
      </Tabs>
    </>
  )
}
