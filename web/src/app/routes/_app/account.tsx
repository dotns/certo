import type { FormEvent } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { useChangePasswordMutation, useDeleteAccountMutation, useProfileQuery } from '@/features/auth/hooks'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { ErrorAlert } from '@/shared/components/error-alert'
import { PageHeader } from '@/shared/components/page-header'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'

export const Route = createFileRoute('/_app/account')({
  component: AccountPage,
})

function AccountPage() {
  const profile = useProfileQuery()
  const change = useChangePasswordMutation()
  const remove = useDeleteAccountMutation()
  const navigate = useNavigate()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [repeat, setRepeat] = useState('')
  const [confirmName, setConfirmName] = useState('')
  const username = profile.data?.username ?? ''

  useEffect(() => {
    document.title = 'Account · certo'
  }, [])

  const mismatch = repeat.length > 0 && next !== repeat

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (mismatch)
      return
    change.mutate({ current_password: current, new_password: next }, {
      onSuccess: () => {
        toast.success('Password changed')
        setCurrent('')
        setNext('')
        setRepeat('')
      },
    })
  }

  return (
    <>
      <PageHeader title="Account" description={username ? `Signed in as ${username}.` : undefined} />
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Change password</CardTitle>
            <CardDescription>Existing sessions and API keys keep working.</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={submit} className="flex flex-col gap-4">
              <Field>
                <FieldLabel htmlFor="current-password">Current password</FieldLabel>
                <Input id="current-password" type="password" autoComplete="current-password" required value={current} onChange={e => setCurrent(e.target.value)} />
              </Field>
              <Field>
                <FieldLabel htmlFor="new-password">New password</FieldLabel>
                <Input id="new-password" type="password" autoComplete="new-password" required minLength={6} value={next} onChange={e => setNext(e.target.value)} />
                <FieldDescription>At least 6 characters.</FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="repeat-password">Repeat new password</FieldLabel>
                <Input id="repeat-password" type="password" autoComplete="new-password" required aria-invalid={mismatch || undefined} value={repeat} onChange={e => setRepeat(e.target.value)} />
                {mismatch && <FieldDescription className="text-destructive">The passwords do not match.</FieldDescription>}
              </Field>
              <ErrorAlert error={change.error} />
              <Button type="submit" className="self-start" disabled={change.isPending || mismatch}>Change password</Button>
            </form>
          </CardContent>
        </Card>
        <Card className="border-destructive/40">
          <CardHeader>
            <CardTitle>Delete account</CardTitle>
            <CardDescription>Removes your API keys, challenge domains, published tokens and hosted zones. Certificates already issued stay valid, but renewals through this account stop.</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Field>
              <FieldLabel htmlFor="confirm-username">Type your username to confirm</FieldLabel>
              <Input id="confirm-username" autoComplete="off" placeholder={username} value={confirmName} onChange={e => setConfirmName(e.target.value)} />
            </Field>
            <ConfirmDialog
              trigger={<Button type="button" variant="destructive" className="self-start" disabled={!username || confirmName !== username}>Delete my account</Button>}
              title="Delete this account?"
              description="This cannot be undone. Hosted zones stop resolving immediately."
              confirmLabel="Delete account"
              onConfirm={async () => {
                await remove.mutateAsync()
                navigate({ to: '/login' })
              }}
            />
          </CardContent>
        </Card>
      </div>
    </>
  )
}
