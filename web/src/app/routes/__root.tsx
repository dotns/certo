import { createRootRoute, Outlet } from '@tanstack/react-router'

export const Route = createRootRoute({
  component: () => <Outlet />,
  notFoundComponent: () => (
    <div className="flex min-h-svh items-center justify-center p-6 text-sm text-muted-foreground">Page not found.</div>
  ),
})
