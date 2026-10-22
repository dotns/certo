import { PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { SearchInput } from '@/shared/components/search-input'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Field, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { matches } from '@/shared/lib/filter'
import { useAddDomainForMutation, useAdminDomainsQuery, useAdminUsersQuery, useRemoveDomainEverywhereMutation } from './hooks'

export function AdminDomains() {
  const domains = useAdminDomainsQuery()
  const users = useAdminUsersQuery()
  const add = useAddDomainForMutation()
  const remove = useRemoveDomainEverywhereMutation()
  const [userId, setUserId] = useState('')
  const [domain, setDomain] = useState('')
  const [query, setQuery] = useState('')
  const visible = (domains.data ?? []).filter(d => matches(query, d.domain, d.owner, d.cname_target))

  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SearchInput value={query} onChange={setQuery} placeholder="Filter domains…" />
        <FormDialog
          trigger={(
            <Button size="sm">
              <PlusIcon data-icon="inline-start" />
              Add domain for user
            </Button>
          )}
          title="Add challenge domain"
          submitLabel="Add"
          onSubmit={async () => {
            await add.mutateAsync({ user_id: Number(userId), domain: domain.trim().toLowerCase() })
            toast.success(`${domain.trim().toLowerCase()} added`)
          }}
          onClose={() => {
            setUserId('')
            setDomain('')
          }}
        >
          <Field>
            <FieldLabel htmlFor="domain-user">User</FieldLabel>
            <Select value={userId} items={(users.data ?? []).map(u => ({ value: String(u.id), label: u.username }))} onValueChange={v => setUserId(String(v ?? ''))}>
              <SelectTrigger id="domain-user" className="w-full">
                <SelectValue placeholder="Choose a user" />
              </SelectTrigger>
              <SelectContent>
                {(users.data ?? []).map(u => <SelectItem key={u.id} value={String(u.id)}>{u.username}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field>
            <FieldLabel htmlFor="domain-name">Domain</FieldLabel>
            <Input id="domain-name" placeholder="example.com" required value={domain} onChange={e => setDomain(e.target.value)} />
          </Field>
        </FormDialog>
      </div>
      <ErrorAlert error={domains.error} title="Domains could not be loaded" />
      {domains.isPending && <Loading />}
      {domains.data && domains.data.length === 0 && <EmptyState title="No challenge domains" />}
      {domains.data && domains.data.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">No domain matches the filter.</p>}
      {visible.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Domain</TableHead>
                  <TableHead>Owner</TableHead>
                  <TableHead>CNAME target</TableHead>
                  <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.map(d => (
                  <TableRow key={`${d.owner}/${d.domain}`}>
                    <TableCell className="font-medium">{d.domain}</TableCell>
                    <TableCell className="text-muted-foreground">{d.owner}</TableCell>
                    <TableCell className="font-mono text-xs">{d.cname_target}</TableCell>
                    <TableCell>
                      <ConfirmDialog
                        trigger={<Button variant="ghost" size="icon-sm" aria-label={`Remove ${d.domain} for every user`}><Trash2Icon /></Button>}
                        title={`Remove ${d.domain}?`}
                        description="The domain is removed from every account that has it, with its tokens."
                        confirmLabel="Remove"
                        onConfirm={async () => {
                          await remove.mutateAsync(d.domain)
                          toast.success(`${d.domain} removed from every account`)
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
