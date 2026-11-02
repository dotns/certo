import { BroomIcon, KeyRoundIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { useZonesQuery } from '@/features/zones/hooks'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { SearchInput } from '@/shared/components/search-input'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Checkbox } from '@/shared/components/ui/checkbox'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { matches } from '@/shared/lib/filter'
import { formatUnix } from '@/shared/lib/format'
import { countByOwner } from './counts'
import { useAdminDomainsQuery, useAdminUsersQuery, useCleanupMutation, useCleanupPreviewQuery, useCreateUserMutation, useDeleteUserMutation, useDeleteUsersMutation, useResetPasswordMutation } from './hooks'

export function AdminUsers() {
  const { t } = useTranslation()
  const users = useAdminUsersQuery()
  const domains = useAdminDomainsQuery()
  const zones = useZonesQuery('admin')
  const create = useCreateUserMutation()
  const remove = useDeleteUserMutation()
  const removeMany = useDeleteUsersMutation()
  const cleanup = useCleanupMutation()
  const reset = useResetPasswordMutation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<Set<number>>(() => new Set())
  const [days, setDays] = useState(7)
  const preview = useCleanupPreviewQuery(days)
  const domainCounts = countByOwner(domains.data)
  const zoneCounts = countByOwner(zones.data)
  const visible = (users.data ?? []).filter(u => matches(query, u.username, u.id))
  const chosen = visible.filter(u => selected.has(u.id))
  const allChosen = visible.length > 0 && chosen.length === visible.length
  const toggle = (id: number, on: boolean) => setSelected((prev) => {
    const next = new Set(prev)
    if (on)
      next.add(id)
    else next.delete(id)
    return next
  })
  const toggleAll = (on: boolean) => setSelected((prev) => {
    const next = new Set(prev)
    for (const u of visible) {
      if (on)
        next.add(u.id)
      else next.delete(u.id)
    }
    return next
  })

  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SearchInput value={query} onChange={setQuery} placeholder={t('admin.users.filter')} />
        <div className="flex flex-wrap items-center gap-2">
          {chosen.length > 0 && (
            <ConfirmDialog
              trigger={(
                <Button size="sm" variant="destructive">
                  <Trash2Icon data-icon="inline-start" />
                  {t('admin.users.deleteSelected', { count: chosen.length })}
                </Button>
              )}
              title={t('admin.users.bulkDelete.title', { count: chosen.length })}
              description={t('admin.users.bulkDelete.description')}
              confirmLabel={t('admin.users.bulkDelete.confirm')}
              onConfirm={async () => {
                const r = await removeMany.mutateAsync(chosen.map(u => u.id))
                setSelected(new Set())
                toast.success(t('admin.users.bulkDelete.done', { count: r.deleted }))
              }}
            />
          )}
          <FormDialog
            trigger={(
              <Button size="sm" variant="outline">
                <BroomIcon data-icon="inline-start" />
                {t('admin.users.cleanup.button')}
              </Button>
            )}
            title={t('admin.users.cleanup.title')}
            description={t('admin.users.cleanup.description')}
            submitLabel={t('admin.users.cleanup.submit')}
            onSubmit={async () => {
              const r = await cleanup.mutateAsync(days)
              toast.success(t('admin.users.cleanup.done', { count: r.deleted }))
            }}
          >
            <Field>
              <FieldLabel htmlFor="cleanup-days">{t('admin.users.cleanup.days')}</FieldLabel>
              <Input id="cleanup-days" type="number" min={0} required value={days} onChange={e => setDays(Math.max(0, Number(e.target.value) || 0))} />
              <FieldDescription>
                {preview.data ? t('admin.users.cleanup.preview', { count: preview.data.accounts }) : '…'}
              </FieldDescription>
            </Field>
          </FormDialog>
          <FormDialog
            trigger={(
              <Button size="sm">
                <PlusIcon data-icon="inline-start" />
                {t('admin.users.create')}
              </Button>
            )}
            title={t('admin.users.dialog.title')}
            submitLabel={t('admin.users.dialog.submit')}
            onSubmit={async () => {
              await create.mutateAsync({ username: username.trim(), password })
              toast.success(t('admin.users.created', { name: username.trim() }))
            }}
            onClose={() => {
              setUsername('')
              setPassword('')
            }}
          >
            <Field>
              <FieldLabel htmlFor="new-username">{t('admin.users.dialog.username')}</FieldLabel>
              <Input id="new-username" required autoFocus autoComplete="off" value={username} onChange={e => setUsername(e.target.value)} />
            </Field>
            <Field>
              <FieldLabel htmlFor="new-password">{t('admin.users.dialog.password')}</FieldLabel>
              <Input id="new-password" type="password" required minLength={6} autoComplete="new-password" value={password} onChange={e => setPassword(e.target.value)} />
            </Field>
          </FormDialog>
        </div>
      </div>
      <ErrorAlert error={users.error} title={t('admin.users.loadError')} />
      {users.isPending && <Loading />}
      {users.data && users.data.length === 0 && <EmptyState title={t('admin.users.empty')} />}
      {users.data && users.data.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">{t('admin.users.noMatch')}</p>}
      {visible.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-0">
                    <Checkbox aria-label={t('admin.users.selectAll')} checked={allChosen} indeterminate={chosen.length > 0 && !allChosen} onCheckedChange={on => toggleAll(on)} />
                  </TableHead>
                  <TableHead>{t('admin.users.columns.username')}</TableHead>
                  <TableHead>{t('admin.users.columns.domains')}</TableHead>
                  <TableHead>{t('admin.users.columns.zones')}</TableHead>
                  <TableHead>{t('admin.users.columns.created')}</TableHead>
                  <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.map(u => (
                  <TableRow key={u.id} data-state={selected.has(u.id) ? 'selected' : undefined}>
                    <TableCell>
                      <Checkbox aria-label={t('admin.users.select', { name: u.username })} checked={selected.has(u.id)} onCheckedChange={on => toggle(u.id, on)} />
                    </TableCell>
                    <TableCell className="font-medium">{u.username}</TableCell>
                    <TableCell className="tabular-nums">{domainCounts[u.username] ?? 0}</TableCell>
                    <TableCell className="tabular-nums">{zoneCounts[u.username] ?? 0}</TableCell>
                    <TableCell className="text-muted-foreground">{formatUnix(u.created_at)}</TableCell>
                    <TableCell>
                      <div className="flex items-center justify-end gap-1">
                        <FormDialog
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={t('admin.users.reset.label', { name: u.username })}><KeyRoundIcon /></Button>}
                          title={t('admin.users.reset.title', { name: u.username })}
                          description={t('admin.users.reset.description')}
                          submitLabel={t('admin.users.reset.submit')}
                          onSubmit={async () => {
                            await reset.mutateAsync({ id: u.id, password: newPassword })
                            toast.success(t('admin.users.reset.done', { name: u.username }))
                          }}
                          onClose={() => setNewPassword('')}
                        >
                          <Field>
                            <FieldLabel htmlFor={`reset-${u.id}`}>{t('admin.users.reset.new')}</FieldLabel>
                            <Input id={`reset-${u.id}`} type="password" required minLength={6} autoComplete="new-password" autoFocus value={newPassword} onChange={e => setNewPassword(e.target.value)} />
                            <FieldDescription>{t('admin.users.reset.hint')}</FieldDescription>
                          </Field>
                        </FormDialog>
                        <ConfirmDialog
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={t('admin.users.delete.label', { name: u.username })}><Trash2Icon /></Button>}
                          title={t('admin.users.delete.title', { name: u.username })}
                          description={t('admin.users.delete.description')}
                          confirmLabel={t('admin.users.delete.confirm')}
                          onConfirm={async () => {
                            await remove.mutateAsync(u.id)
                            toast.success(t('admin.users.delete.done', { name: u.username }))
                          }}
                        />
                      </div>
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
