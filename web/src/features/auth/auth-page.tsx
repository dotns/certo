import type { FormEvent, ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ErrorAlert } from '@/shared/components/error-alert'
import { LanguageToggle } from '@/shared/components/language-toggle'
import { ModeToggle } from '@/shared/components/mode-toggle'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Field, FieldGroup, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'

interface AuthPageProps {
  title: string
  description: string
  submitLabel: string
  busyLabel: string
  footer: ReactNode
  error: unknown
  busy: boolean
  minPassword?: number
  passwordPlaceholder?: string
  onSubmit: (credentials: { username: string, password: string }) => void
}

/** Shared layout for the sign-in and sign-up forms. */
export function AuthPage({ title, description, submitLabel, busyLabel, footer, error, busy, minPassword, passwordPlaceholder, onSubmit }: AuthPageProps) {
  const { t } = useTranslation()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')

  const submit = (e: FormEvent) => {
    e.preventDefault()
    onSubmit({ username: username.trim(), password })
  }

  return (
    <div className="flex min-h-svh flex-col items-center justify-center gap-6 bg-muted/40 p-6">
      <div className="flex w-full max-w-sm items-center justify-between">
        <Link to="/" className="flex items-center gap-2 font-heading text-lg font-semibold">
          <img src="/favicon.svg" alt="" className="size-6" />
          {t('common.appName')}
        </Link>
        <div className="flex items-center gap-1">
          <LanguageToggle />
          <ModeToggle />
        </div>
      </div>
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submit} className="flex flex-col gap-4">
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="username">{t('auth.username')}</FieldLabel>
                <Input id="username" name="username" autoComplete="username" required autoFocus value={username} onChange={e => setUsername(e.target.value)} />
              </Field>
              <Field>
                <FieldLabel htmlFor="password">{t('auth.password')}</FieldLabel>
                <Input id="password" name="password" type="password" autoComplete={minPassword ? 'new-password' : 'current-password'} required minLength={minPassword} placeholder={passwordPlaceholder} value={password} onChange={e => setPassword(e.target.value)} />
              </Field>
            </FieldGroup>
            <ErrorAlert error={error} />
            <Button type="submit" disabled={busy}>{busy ? busyLabel : submitLabel}</Button>
          </form>
        </CardContent>
      </Card>
      <p className="text-sm text-muted-foreground">{footer}</p>
    </div>
  )
}
