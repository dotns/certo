import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { AuthPage } from '@/features/auth/auth-page'
import { useRegisterMutation } from '@/features/auth/hooks'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/register')({
  beforeLoad: () => {
    if (useSession.getState().token)
      throw redirect({ to: '/overview' })
  },
  component: RegisterPage,
})

function RegisterPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const register = useRegisterMutation()

  useEffect(() => {
    document.title = t('titles.register')
  }, [t])

  return (
    <AuthPage
      title={t('auth.register.title')}
      description={t('auth.register.description')}
      submitLabel={t('auth.register.submit')}
      busyLabel={t('auth.register.busy')}
      minPassword={6}
      passwordPlaceholder={t('auth.register.passwordPlaceholder')}
      error={register.error}
      busy={register.isPending}
      onSubmit={credentials => register.mutate(credentials, { onSuccess: () => navigate({ to: '/overview' }) })}
      footer={(
        <>
          {t('auth.register.already')}
          {' '}
          <Link to="/login" className="underline underline-offset-4 hover:text-foreground">{t('auth.register.signIn')}</Link>
        </>
      )}
    />
  )
}
