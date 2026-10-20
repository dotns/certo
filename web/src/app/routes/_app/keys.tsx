import { createFileRoute } from '@tanstack/react-router'
import { PlusIcon, Trash2Icon, XIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useCreateKeyMutation, useDeleteKeyMutation, useKeysQuery } from '@/features/keys/hooks'
import { isGlobalScope, normalizeScopes } from '@/features/keys/scopes'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { CopyButton } from '@/shared/components/copy-button'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { Badge } from '@/shared/components/ui/badge'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { abbreviate, formatUnix } from '@/shared/lib/format'

export const Route = createFileRoute('/_app/keys')({
  component: KeysPage,
})

function KeysPage() {
  const keys = useKeysQuery()
  const create = useCreateKeyMutation()
  const remove = useDeleteKeyMutation()
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<string[]>([''])

  useEffect(() => {
    document.title = 'API keys · certo'
  }, [])

  const reset = () => {
    setName('')
    setScopes([''])
  }

  return (
    <>
      <PageHeader
        title="API keys"
        description="Keys authenticate ACME clients (httpreq and acme-dns). A scoped key only reaches the domains it lists."
        actions={(
          <FormDialog
            trigger={(
              <Button>
                <PlusIcon data-icon="inline-start" />
                Create key
              </Button>
            )}
            title="Create API key"
            submitLabel="Create key"
            onSubmit={() => create.mutateAsync({ name: name.trim(), scope: normalizeScopes(scopes) })}
            onClose={reset}
          >
            <Field>
              <FieldLabel htmlFor="key-name">Name</FieldLabel>
              <Input id="key-name" placeholder="traefik-prod" required autoFocus value={name} onChange={e => setName(e.target.value)} />
            </Field>
            <Field>
              <FieldLabel>Domains</FieldLabel>
              <div className="flex flex-col gap-2">
                {scopes.map((s, i) => (
                  // Rows are positional inputs; the index is the identity.
                  // eslint-disable-next-line react/no-array-index-key
                  <div key={i} className="flex items-center gap-2">
                    <Input aria-label={`Domain ${i + 1}`} placeholder="*.example.com" value={s} onChange={e => setScopes(scopes.map((v, j) => (j === i ? e.target.value : v)))} />
                    {scopes.length > 1 && (
                      <Button type="button" variant="ghost" size="icon-sm" aria-label={`Remove domain ${i + 1}`} onClick={() => setScopes(scopes.filter((_, j) => j !== i))}>
                        <XIcon />
                      </Button>
                    )}
                  </div>
                ))}
                <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => setScopes([...scopes, ''])}>
                  <PlusIcon data-icon="inline-start" />
                  Add domain
                </Button>
              </div>
              <FieldDescription>
                <code>*.example.com</code>
                {' '}
                covers the domain and its subdomains,
                {' '}
                <code>example.com</code>
                {' '}
                the exact name. Leave empty for a global key.
              </FieldDescription>
            </Field>
          </FormDialog>
        )}
      />
      <ErrorAlert error={keys.error} title="Keys could not be loaded" />
      {keys.isPending && <Loading />}
      {keys.data && keys.data.length === 0 && <EmptyState title="No API keys" description="Create a key to configure an ACME client." />}
      {keys.data && keys.data.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Key</TableHead>
                  <TableHead>Scope</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keys.data.map(k => (
                  <TableRow key={k.id}>
                    <TableCell className="font-medium">{k.name}</TableCell>
                    <TableCell>
                      <span className="inline-flex items-center gap-1 font-mono text-xs" title={k.key}>
                        {abbreviate(k.key, 8)}
                        <CopyButton text={k.key} label="" />
                      </span>
                    </TableCell>
                    <TableCell>
                      {isGlobalScope(k.scope)
                        ? <Badge variant="secondary">Global</Badge>
                        : <span className="font-mono text-xs">{k.scope.join(', ')}</span>}
                    </TableCell>
                    <TableCell className="text-muted-foreground">{formatUnix(k.created_at)}</TableCell>
                    <TableCell>
                      <ConfirmDialog
                        trigger={<Button variant="ghost" size="icon-sm" aria-label={`Delete key ${k.name}`}><Trash2Icon /></Button>}
                        title={`Delete key "${k.name}"?`}
                        description="Clients using this key stop working immediately."
                        confirmLabel="Delete"
                        onConfirm={() => remove.mutateAsync(k.id)}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
    </>
  )
}
