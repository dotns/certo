import { createFileRoute, redirect } from '@tanstack/react-router'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/')({
  beforeLoad: () => {
    throw redirect({ to: useSession.getState().token ? '/overview' : '/login' })
  },
})
