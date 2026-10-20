import { createFileRoute, Link, Navigate, Outlet, redirect } from '@tanstack/react-router'
import { LogOutIcon } from 'lucide-react'
import { useProfileQuery, useSignOut } from '@/features/auth/hooks'
import { useInfoQuery } from '@/features/info/hooks'
import { ModeToggle } from '@/shared/components/mode-toggle'
import { Button } from '@/shared/components/ui/button'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/_app')({
  beforeLoad: () => {
    if (!useSession.getState().token)
      throw redirect({ to: '/login' })
  },
  component: AppLayout,
})

const NAV = [
  { to: '/domains', label: 'Domains' },
  { to: '/zones', label: 'Zones' },
  { to: '/keys', label: 'API keys' },
  { to: '/config', label: 'Clients' },
  { to: '/manage', label: 'Admin' },
] as const

function AppLayout() {
  const token = useSession(s => s.token)
  const signOut = useSignOut()
  const profile = useProfileQuery()
  const info = useInfoQuery()

  if (!token)
    return <Navigate to="/login" />

  return (
    <div className="flex min-h-svh flex-col">
      <header className="sticky top-0 z-40 border-b bg-background/95 backdrop-blur supports-backdrop-filter:bg-background/80">
        <div className="mx-auto flex w-full max-w-6xl flex-wrap items-center gap-x-6 gap-y-2 px-4 py-2">
          <Link to="/domains" className="flex items-center gap-2 font-heading text-base font-semibold">
            <img src="/favicon.svg" alt="" className="size-6" />
            certo
          </Link>
          <nav aria-label="Main" className="flex flex-1 items-center gap-1 overflow-x-auto">
            {NAV.map(item => (
              <Link
                key={item.to}
                to={item.to}
                className="rounded-md px-2.5 py-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground data-[status=active]:bg-muted data-[status=active]:text-foreground"
              >
                {item.label}
              </Link>
            ))}
          </nav>
          <div className="flex items-center gap-1">
            <span className="hidden text-sm text-muted-foreground sm:inline">{profile.data?.username}</span>
            <ModeToggle />
            <Button type="button" variant="ghost" size="icon-sm" aria-label="Sign out" onClick={signOut}>
              <LogOutIcon />
            </Button>
          </div>
        </div>
      </header>
      <main className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-6 px-4 py-6">
        <Outlet />
      </main>
      <footer className="mx-auto flex w-full max-w-6xl items-center gap-3 px-4 py-4 text-xs text-muted-foreground">
        <a href="https://github.com/dotns/certo" target="_blank" rel="noreferrer" className="hover:text-foreground">GitHub</a>
        <a href="/llms.txt" target="_blank" className="hover:text-foreground">llms.txt</a>
        <span className="ml-auto">{info.data?.version ? `certo ${info.data.version}` : ''}</span>
      </footer>
    </div>
  )
}
