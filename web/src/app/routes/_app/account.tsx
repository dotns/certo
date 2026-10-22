import type { FormEvent } from 'react'
import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation()
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
    document.title = t('titles.account')
  }, [t])

  const mismatch = repeat.length > 0 && next !== repeat

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (mismatch)
      return
    change.mutate({ current_password: current, new_password: next }, {
      onSuccess: () => {
        toast.success(t('account.password.changed'))
        setCurrent('')
        setNext('')
        setRepeat('')
      },
    })
  }

  return (
    <>
      <PageHeader title={t('account.title')} description={username ? t('account.signedInAs', { name: username }) : undefined} />
      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>{t('account.password.title')}</CardTitle>
            <CardDescription>{t('account.password.description')}</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={submit} className="flex flex-col gap-4">
              <Field>
                <FieldLabel htmlFor="current-password">{t('account.password.current')}</FieldLabel>
                <Input id="current-password" type="password" autoComplete="current-password" required value={current} onChange={e => setCurrent(e.target.value)} />
              </Field>
              <Field>
                <FieldLabel htmlFor="new-password">{t('account.password.new')}</FieldLabel>
                <Input id="new-password" type="password" autoComplete="new-password" required minLength={6} value={next} onChange={e => setNext(e.target.value)} />
                <FieldDescription>{t('account.password.newHint')}</FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="repeat-password">{t('account.password.repeat')}</FieldLabel>
                <Input id="repeat-password" type="password" autoComplete="new-password" required aria-invalid={mismatch || undefined} value={repeat} onChange={e => setRepeat(e.target.value)} />
                {mismatch && <FieldDescription className="text-destructive">{t('account.password.mismatch')}</FieldDescription>}
              </Field>
              <ErrorAlert error={change.error} />
              <Button type="submit" className="self-start" disabled={change.isPending || mismatch}>{t('account.password.submit')}</Button>
            </form>
          </CardContent>
        </Card>
        <Card className="border-destructive/40">
          <CardHeader>
            <CardTitle>{t('account.delete.title')}</CardTitle>
            <CardDescription>{t('account.delete.description')}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <Field>
              <FieldLabel htmlFor="confirm-username">{t('account.delete.confirmLabel')}</FieldLabel>
              <Input id="confirm-username" autoComplete="off" placeholder={username} value={confirmName} onChange={e => setConfirmName(e.target.value)} />
            </Field>
            <ConfirmDialog
              trigger={<Button type="button" variant="destructive" className="self-start" disabled={!username || confirmName !== username}>{t('account.delete.button')}</Button>}
              title={t('account.delete.dialogTitle')}
              description={t('account.delete.dialogDescription')}
              confirmLabel={t('account.delete.dialogConfirm')}
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
