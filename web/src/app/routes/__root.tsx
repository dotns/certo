import { createRootRoute, Outlet } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'

export const Route = createRootRoute({
  component: () => <Outlet />,
  notFoundComponent: NotFound,
})

function NotFound() {
  const { t } = useTranslation()
  return <div className="flex min-h-svh items-center justify-center p-6 text-sm text-muted-foreground">{t('common.pageNotFound')}</div>
}
