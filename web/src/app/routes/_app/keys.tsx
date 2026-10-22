import type { ApiKey } from '@/features/keys/api'
import { createFileRoute, Link } from '@tanstack/react-router'
import { PlusIcon, Trash2Icon, XIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { useCreateKeyMutation, useDeleteKeyMutation, useKeysQuery } from '@/features/keys/hooks'
import { isGlobalScope, normalizeScopes } from '@/features/keys/scopes'
import { CodeBlock } from '@/shared/components/code-block'
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
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { abbreviate, formatUnix } from '@/shared/lib/format'

export const Route = createFileRoute('/_app/keys')({
  component: KeysPage,
})

function KeysPage() {
  const { t } = useTranslation()
  const keys = useKeysQuery()
  const create = useCreateKeyMutation()
  const remove = useDeleteKeyMutation()
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<string[]>([''])
  const [created, setCreated] = useState<ApiKey | null>(null)

  useEffect(() => {
    document.title = t('titles.keys')
  }, [t])

  const reset = () => {
    setName('')
    setScopes([''])
  }

  return (
    <>
      <PageHeader
        title={t('keys.title')}
        description={t('keys.description')}
        actions={(
          <FormDialog
            trigger={(
              <Button>
                <PlusIcon data-icon="inline-start" />
                {t('keys.create')}
              </Button>
            )}
            title={t('keys.dialog.title')}
            submitLabel={t('keys.dialog.submit')}
            onSubmit={async () => {
              const key = await create.mutateAsync({ name: name.trim(), scope: normalizeScopes(scopes) })
              setCreated(key)
            }}
            onClose={reset}
          >
            <Field>
              <FieldLabel htmlFor="key-name">{t('keys.dialog.name')}</FieldLabel>
              <Input id="key-name" placeholder="traefik-prod" required autoFocus value={name} onChange={e => setName(e.target.value)} />
            </Field>
            <Field>
              <FieldLabel>{t('keys.dialog.domains')}</FieldLabel>
              <div className="flex flex-col gap-2">
                {scopes.map((s, i) => (
                  // Rows are positional inputs; the index is the identity.
                  // eslint-disable-next-line react/no-array-index-key
                  <div key={i} className="flex items-center gap-2">
                    <Input aria-label={t('keys.dialog.domain', { n: i + 1 })} placeholder="*.example.com" value={s} onChange={e => setScopes(scopes.map((v, j) => (j === i ? e.target.value : v)))} />
                    {scopes.length > 1 && (
                      <Button type="button" variant="ghost" size="icon-sm" aria-label={t('keys.dialog.removeDomain', { n: i + 1 })} onClick={() => setScopes(scopes.filter((_, j) => j !== i))}>
                        <XIcon />
                      </Button>
                    )}
                  </div>
                ))}
                <Button type="button" variant="outline" size="sm" className="self-start" onClick={() => setScopes([...scopes, ''])}>
                  <PlusIcon data-icon="inline-start" />
                  {t('keys.dialog.addDomain')}
                </Button>
              </div>
              <FieldDescription>{t('keys.dialog.hint')}</FieldDescription>
            </Field>
          </FormDialog>
        )}
      />
      <ErrorAlert error={keys.error} title={t('keys.loadError')} />
      {keys.isPending && <Loading />}
      {keys.data && keys.data.length === 0 && <EmptyState title={t('keys.empty.title')} description={t('keys.empty.description')} />}
      {keys.data && keys.data.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('keys.columns.name')}</TableHead>
                  <TableHead>{t('keys.columns.key')}</TableHead>
                  <TableHead>{t('keys.columns.scope')}</TableHead>
                  <TableHead>{t('keys.columns.created')}</TableHead>
                  <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
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
                        ? <Badge variant="secondary">{t('keys.global')}</Badge>
                        : <span className="font-mono text-xs">{k.scope.join(', ')}</span>}
                    </TableCell>
                    <TableCell className="text-muted-foreground">{formatUnix(k.created_at)}</TableCell>
                    <TableCell>
                      <ConfirmDialog
                        trigger={<Button variant="ghost" size="icon-sm" aria-label={t('keys.delete.label', { name: k.name })}><Trash2Icon /></Button>}
                        title={t('keys.delete.title', { name: k.name })}
                        description={t('keys.delete.description')}
                        confirmLabel={t('keys.delete.confirm')}
                        onConfirm={async () => {
                          await remove.mutateAsync(k.id)
                          toast.success(t('keys.delete.done', { name: k.name }))
                        }}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
      <Dialog open={created !== null} onOpenChange={open => !open && setCreated(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('keys.created.title', { name: created?.name ?? '' })}</DialogTitle>
            <DialogDescription>{t('keys.created.description')}</DialogDescription>
          </DialogHeader>
          {created && (
            <div className="flex flex-col gap-3">
              <CodeBlock title={t('keys.created.key')} code={created.key} wrap />
              <Link to="/config" search={{ key: created.id }} className="self-start">
                <Button type="button" variant="outline" size="sm">{t('keys.created.snippets')}</Button>
              </Link>
            </div>
          )}
          <DialogFooter showCloseButton />
        </DialogContent>
      </Dialog>
    </>
  )
}
