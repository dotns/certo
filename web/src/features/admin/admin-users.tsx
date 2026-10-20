import { PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Field, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { formatUnix } from '@/shared/lib/format'
import { useAdminUsersQuery, useCreateUserMutation, useDeleteUserMutation } from './hooks'

export function AdminUsers() {
  const users = useAdminUsersQuery()
  const create = useCreateUserMutation()
  const remove = useDeleteUserMutation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')

  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex justify-end">
        <FormDialog
          trigger={(
            <Button size="sm">
              <PlusIcon data-icon="inline-start" />
              Create user
            </Button>
          )}
          title="Create user"
          submitLabel="Create"
          onSubmit={() => create.mutateAsync({ username: username.trim(), password })}
          onClose={() => {
            setUsername('')
            setPassword('')
          }}
        >
          <Field>
            <FieldLabel htmlFor="new-username">Username</FieldLabel>
            <Input id="new-username" required autoFocus autoComplete="off" value={username} onChange={e => setUsername(e.target.value)} />
          </Field>
          <Field>
            <FieldLabel htmlFor="new-password">Password</FieldLabel>
            <Input id="new-password" type="password" required minLength={6} autoComplete="new-password" value={password} onChange={e => setPassword(e.target.value)} />
          </Field>
        </FormDialog>
      </div>
      <ErrorAlert error={users.error} title="Users could not be loaded" />
      {users.isPending && <Loading />}
      {users.data && users.data.length === 0 && <EmptyState title="No users" />}
      {users.data && users.data.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Username</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {users.data.map(u => (
                  <TableRow key={u.id}>
                    <TableCell className="font-medium">{u.username}</TableCell>
                    <TableCell className="text-muted-foreground">{formatUnix(u.created_at)}</TableCell>
                    <TableCell>
                      <ConfirmDialog
                        trigger={<Button variant="ghost" size="icon-sm" aria-label={`Delete user ${u.username}`}><Trash2Icon /></Button>}
                        title={`Delete ${u.username}?`}
                        description="The account, its keys, domains, tokens and zones are removed."
                        confirmLabel="Delete user"
                        onConfirm={() => remove.mutateAsync(u.id)}
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
