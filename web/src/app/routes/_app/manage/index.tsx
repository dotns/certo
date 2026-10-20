import { createFileRoute } from '@tanstack/react-router'
import { LockIcon } from 'lucide-react'
import { useEffect } from 'react'
import { AdminDomains } from '@/features/admin/admin-domains'
import { AdminGate } from '@/features/admin/admin-gate'
import { AdminRecords } from '@/features/admin/admin-records'
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
  const adminKey = useSession(s => s.adminKey)
  const setAdminKey = useSession(s => s.setAdminKey)

  useEffect(() => {
    document.title = 'Admin · certo'
  }, [])

  return (
    <>
      <PageHeader
        title="Administration"
        description="Users, every hosted zone, challenge domains and published tokens."
        actions={adminKey && (
          <Button type="button" variant="outline" size="sm" onClick={() => setAdminKey(null)}>
            <LockIcon data-icon="inline-start" />
            Lock
          </Button>
        )}
      />
      <AdminGate>
        <Tabs defaultValue="zones">
          <TabsList>
            <TabsTrigger value="zones">Zones</TabsTrigger>
            <TabsTrigger value="users">Users</TabsTrigger>
            <TabsTrigger value="domains">Domains</TabsTrigger>
            <TabsTrigger value="records">TXT records</TabsTrigger>
          </TabsList>
          <TabsContent value="zones"><AdminZones /></TabsContent>
          <TabsContent value="users"><AdminUsers /></TabsContent>
          <TabsContent value="domains"><AdminDomains /></TabsContent>
          <TabsContent value="records"><AdminRecords /></TabsContent>
        </Tabs>
      </AdminGate>
    </>
  )
}
