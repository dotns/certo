import { createFileRoute, Link, Navigate, Outlet, redirect, useNavigate } from '@tanstack/react-router'
import { LogOutIcon, MenuIcon, UserIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useProfileQuery, useSignOut } from '@/features/auth/hooks'
import { useInfoQuery } from '@/features/info/hooks'
import { LanguageToggle } from '@/shared/components/language-toggle'
import { ModeToggle } from '@/shared/components/mode-toggle'
import { Button } from '@/shared/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from '@/shared/components/ui/dropdown-menu'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/_app')({
  beforeLoad: () => {
    if (!useSession.getState().token)
      throw redirect({ to: '/login' })
  },
  component: AppLayout,
})

const NAV = [
  { to: '/overview', key: 'nav.overview' },
  { to: '/domains', key: 'nav.domains' },
  { to: '/zones', key: 'nav.zones' },
  { to: '/keys', key: 'nav.keys' },
  { to: '/config', key: 'nav.clients' },
  { to: '/manage', key: 'nav.admin' },
] as const

function AppLayout() {
  const { t } = useTranslation()
  const token = useSession(s => s.token)
  const signOut = useSignOut()
  const navigate = useNavigate()
  const profile = useProfileQuery()
  const info = useInfoQuery()

  if (!token)
    return <Navigate to="/login" />

  const username = profile.data?.username ?? t('nav.account')

  return (
    <div className="flex min-h-svh flex-col">
      <header className="sticky top-0 z-40 border-b bg-background/95 backdrop-blur supports-backdrop-filter:bg-background/80">
        <div className="mx-auto flex w-full max-w-6xl items-center gap-x-4 px-4 py-2">
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="icon-sm" className="md:hidden" aria-label={t('nav.openNavigation')} />}>
              <MenuIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-44">
              {NAV.map(item => (
                <DropdownMenuItem key={item.to} onClick={() => navigate({ to: item.to })}>{t(item.key)}</DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <Link to="/overview" className="flex items-center gap-2 font-heading text-base font-semibold">
            <img src="/favicon.svg" alt="" className="size-6" />
            {t('common.appName')}
          </Link>
          <nav aria-label={t('nav.main')} className="hidden flex-1 items-center gap-1 md:flex">
            {NAV.map(item => (
              <Link
                key={item.to}
                to={item.to}
                className="rounded-md px-2.5 py-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground data-[status=active]:bg-muted data-[status=active]:text-foreground"
              >
                {t(item.key)}
              </Link>
            ))}
          </nav>
          <div className="ml-auto flex items-center gap-1">
            <LanguageToggle />
            <ModeToggle />
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button type="button" variant="ghost" size="sm" aria-label={t('nav.accountMenu')} />}>
                <UserIcon data-icon="inline-start" />
                <span className="hidden sm:inline">{username}</span>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-44">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>{username}</DropdownMenuLabel>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => navigate({ to: '/account' })}>
                  <UserIcon />
                  {t('nav.account')}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={signOut}>
                  <LogOutIcon />
                  {t('nav.signOut')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </header>
      <main className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-6 px-4 py-6">
        <Outlet />
      </main>
      <footer className="mx-auto flex w-full max-w-6xl items-center gap-3 px-4 py-4 text-xs text-muted-foreground">
        <a href="https://github.com/dotns/certo" target="_blank" rel="noreferrer" className="hover:text-foreground">{t('common.github')}</a>
        <a href="/llms.txt" target="_blank" className="hover:text-foreground">{t('common.llms')}</a>
        <span className="ml-auto">{info.data?.version ? t('common.version', { version: info.data.version }) : ''}</span>
      </footer>
    </div>
  )
}
