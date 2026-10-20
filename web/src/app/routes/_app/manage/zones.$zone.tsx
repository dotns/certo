import { createFileRoute } from '@tanstack/react-router'
import { AdminGate } from '@/features/admin/admin-gate'
import { ZoneDetail } from '@/features/zones/zone-detail'

export const Route = createFileRoute('/_app/manage/zones/$zone')({
  component: AdminZonePage,
})

function AdminZonePage() {
  const { zone } = Route.useParams()
  return (
    <AdminGate>
      <ZoneDetail scope="admin" zone={zone} />
    </AdminGate>
  )
}
