import { createFileRoute, Link, redirect, useNavigate } from '@tanstack/react-router'
import { useEffect } from 'react'
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
  const navigate = useNavigate()
  const login = useLoginMutation()

  useEffect(() => {
    document.title = 'Sign in · certo'
  }, [])

  return (
    <AuthPage
      title="Sign in"
      description="Manage challenge domains, API keys and hosted zones."
      submitLabel="Sign in"
      busyLabel="Signing in…"
      error={login.error}
      busy={login.isPending}
      onSubmit={credentials => login.mutate(credentials, { onSuccess: () => navigate({ to: '/overview' }) })}
      footer={(
        <>
          No account yet?
          {' '}
          <Link to="/register" className="underline underline-offset-4 hover:text-foreground">Create one</Link>
        </>
      )}
    />
  )
}
