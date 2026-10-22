import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { AuthPage } from '@/features/auth/auth-page'
import { useLoginMutation } from '@/features/auth/hooks'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/login')({
  beforeLoad: () => {
    if (useSession.getState().token)
      throw redirect({ to: '/overview' })
  },
  component: LoginPage,
})

function LoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const login = useLoginMutation()

  useEffect(() => {
    document.title = t('titles.signIn')
  }, [t])

  return (
    <AuthPage
      title={t('auth.signIn.title')}
      description={t('auth.signIn.description')}
      submitLabel={t('auth.signIn.submit')}
      busyLabel={t('auth.signIn.busy')}
      error={login.error}
      busy={login.isPending}
      onSubmit={credentials => login.mutate(credentials, { onSuccess: () => navigate({ to: '/overview' }) })}
      footer={(
        <>
          {t('auth.signIn.noAccount')}
          {' '}
          <Link to="/register" className="underline underline-offset-4 hover:text-foreground">{t('auth.signIn.create')}</Link>
        </>
      )}
    />
  )
}
