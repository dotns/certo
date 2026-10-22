import { createFileRoute } from '@tanstack/react-router'
import { LockIcon } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { AdminDomains } from '@/features/admin/admin-domains'
import { AdminGate } from '@/features/admin/admin-gate'
import { AdminRecords } from '@/features/admin/admin-records'
import { AdminServer } from '@/features/admin/admin-server'
import { AdminUsers } from '@/features/admin/admin-users'
import { AdminZones } from '@/features/admin/admin-zones'
import { PageHeader } from '@/shared/components/page-header'
import { Button } from '@/shared/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/components/ui/tabs'
import { useSession } from '@/shared/lib/session'

export const Route = createFileRoute('/_app/manage/')({
  component: AdminPage,
})

function AdminPage() {
  const { t } = useTranslation()
  const adminKey = useSession(s => s.adminKey)
  const setAdminKey = useSession(s => s.setAdminKey)

  useEffect(() => {
    document.title = t('titles.admin')
  }, [t])

  return (
    <>
      <PageHeader
        title={t('admin.title')}
        description={t('admin.description')}
        actions={adminKey && (
          <Button type="button" variant="outline" size="sm" onClick={() => setAdminKey(null)}>
            <LockIcon data-icon="inline-start" />
            {t('admin.lock')}
          </Button>
        )}
      />
      <AdminGate>
        <Tabs defaultValue="zones">
          <TabsList>
            <TabsTrigger value="zones">{t('admin.tabs.zones')}</TabsTrigger>
            <TabsTrigger value="users">{t('admin.tabs.users')}</TabsTrigger>
            <TabsTrigger value="domains">{t('admin.tabs.domains')}</TabsTrigger>
            <TabsTrigger value="records">{t('admin.tabs.records')}</TabsTrigger>
            <TabsTrigger value="server">{t('admin.tabs.server')}</TabsTrigger>
          </TabsList>
          <TabsContent value="zones"><AdminZones /></TabsContent>
          <TabsContent value="users"><AdminUsers /></TabsContent>
          <TabsContent value="domains"><AdminDomains /></TabsContent>
          <TabsContent value="records"><AdminRecords /></TabsContent>
          <TabsContent value="server"><AdminServer /></TabsContent>
        </Tabs>
      </AdminGate>
    </>
  )
}
