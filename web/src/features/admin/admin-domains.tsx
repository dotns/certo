import { PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { SearchInput } from '@/shared/components/search-input'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Checkbox } from '@/shared/components/ui/checkbox'
import { Field, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { matches } from '@/shared/lib/filter'
import { useAddDomainForMutation, useAdminDomainsQuery, useAdminUsersQuery, useDeleteDomainsMutation, useRemoveDomainEverywhereMutation } from './hooks'

export function AdminDomains() {
  const { t } = useTranslation()
  const domains = useAdminDomainsQuery()
  const users = useAdminUsersQuery()
  const add = useAddDomainForMutation()
  const remove = useRemoveDomainEverywhereMutation()
  const removeMany = useDeleteDomainsMutation()
  const [userId, setUserId] = useState('')
  const [domain, setDomain] = useState('')
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<Set<string>>(() => new Set())
  const visible = (domains.data ?? []).filter(d => matches(query, d.domain, d.owner, d.cname_target))
  const chosen = [...new Set(visible.filter(d => selected.has(d.domain)).map(d => d.domain))]
  const allChosen = visible.length > 0 && visible.every(d => selected.has(d.domain))
  const toggle = (name: string, on: boolean) => setSelected((prev) => {
    const next = new Set(prev)
    if (on)
      next.add(name)
    else next.delete(name)
    return next
  })
  const toggleAll = (on: boolean) => setSelected((prev) => {
    const next = new Set(prev)
    for (const d of visible) {
      if (on)
        next.add(d.domain)
      else next.delete(d.domain)
    }
    return next
  })

  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SearchInput value={query} onChange={setQuery} placeholder={t('admin.domains.filter')} />
        <div className="flex flex-wrap items-center gap-2">
          {chosen.length > 0 && (
            <ConfirmDialog
              trigger={(
                <Button size="sm" variant="destructive">
                  <Trash2Icon data-icon="inline-start" />
                  {t('admin.domains.deleteSelected', { count: chosen.length })}
                </Button>
              )}
              title={t('admin.domains.bulkDelete.title', { count: chosen.length })}
              description={t('admin.domains.bulkDelete.description')}
              confirmLabel={t('admin.domains.bulkDelete.confirm')}
              onConfirm={async () => {
                const r = await removeMany.mutateAsync(chosen)
                setSelected(new Set())
                toast.success(t('admin.domains.bulkDelete.done', { count: r.deleted }))
              }}
            />
          )}
          <FormDialog
            trigger={(
              <Button size="sm">
                <PlusIcon data-icon="inline-start" />
                {t('admin.domains.add')}
              </Button>
            )}
            title={t('admin.domains.dialog.title')}
            submitLabel={t('admin.domains.dialog.submit')}
            onSubmit={async () => {
              await add.mutateAsync({ user_id: Number(userId), domain: domain.trim().toLowerCase() })
              toast.success(t('admin.domains.added', { domain: domain.trim().toLowerCase() }))
            }}
            onClose={() => {
              setUserId('')
              setDomain('')
            }}
          >
            <Field>
              <FieldLabel htmlFor="domain-user">{t('admin.domains.dialog.user')}</FieldLabel>
              <Select value={userId} items={(users.data ?? []).map(u => ({ value: String(u.id), label: u.username }))} onValueChange={v => setUserId(String(v ?? ''))}>
                <SelectTrigger id="domain-user" className="w-full">
                  <SelectValue placeholder={t('admin.domains.dialog.chooseUser')} />
                </SelectTrigger>
                <SelectContent>
                  {(users.data ?? []).map(u => <SelectItem key={u.id} value={String(u.id)}>{u.username}</SelectItem>)}
                </SelectContent>
              </Select>
            </Field>
            <Field>
              <FieldLabel htmlFor="domain-name">{t('admin.domains.dialog.domain')}</FieldLabel>
              <Input id="domain-name" placeholder="example.com" required value={domain} onChange={e => setDomain(e.target.value)} />
            </Field>
          </FormDialog>
        </div>
      </div>
      <ErrorAlert error={domains.error} title={t('admin.domains.loadError')} />
      {domains.isPending && <Loading />}
      {domains.data && domains.data.length === 0 && <EmptyState title={t('admin.domains.empty')} />}
      {domains.data && domains.data.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">{t('admin.domains.noMatch')}</p>}
      {visible.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-0">
                    <Checkbox aria-label={t('admin.domains.selectAll')} checked={allChosen} indeterminate={chosen.length > 0 && !allChosen} onCheckedChange={on => toggleAll(on)} />
                  </TableHead>
                  <TableHead>{t('admin.domains.columns.domain')}</TableHead>
                  <TableHead>{t('admin.domains.columns.owner')}</TableHead>
                  <TableHead>{t('admin.domains.columns.target')}</TableHead>
                  <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.map(d => (
                  <TableRow key={`${d.owner}/${d.domain}`} data-state={selected.has(d.domain) ? 'selected' : undefined}>
                    <TableCell>
                      <Checkbox aria-label={t('admin.domains.select', { domain: d.domain })} checked={selected.has(d.domain)} onCheckedChange={on => toggle(d.domain, on)} />
                    </TableCell>
                    <TableCell className="font-medium">{d.domain}</TableCell>
                    <TableCell className="text-muted-foreground">{d.owner}</TableCell>
                    <TableCell className="font-mono text-xs">{d.cname_target}</TableCell>
                    <TableCell>
                      <ConfirmDialog
                        trigger={<Button variant="ghost" size="icon-sm" aria-label={t('admin.domains.remove.label', { domain: d.domain })}><Trash2Icon /></Button>}
                        title={t('admin.domains.remove.title', { domain: d.domain })}
                        description={t('admin.domains.remove.description')}
                        confirmLabel={t('admin.domains.remove.confirm')}
                        onConfirm={async () => {
                          await remove.mutateAsync(d.domain)
                          toast.success(t('admin.domains.remove.done', { domain: d.domain }))
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
    </div>
  )
}
