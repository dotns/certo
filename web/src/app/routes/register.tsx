import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'
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
  const navigate = useNavigate()
  const register = useRegisterMutation()

  useEffect(() => {
    document.title = 'Create account · certo'
  }, [])

  return (
    <AuthPage
      title="Create account"
      description="Your account comes with a global API key for ACME clients."
      submitLabel="Create account"
      busyLabel="Creating…"
      minPassword={6}
      passwordPlaceholder="At least 6 characters"
      error={register.error}
      busy={register.isPending}
      onSubmit={credentials => register.mutate(credentials, { onSuccess: () => navigate({ to: '/overview' }) })}
      footer={(
        <>
          Already registered?
          {' '}
          <Link to="/login" className="underline underline-offset-4 hover:text-foreground">Sign in</Link>
        </>
      )}
    />
  )
}
