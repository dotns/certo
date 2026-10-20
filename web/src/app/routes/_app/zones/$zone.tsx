import { createFileRoute } from '@tanstack/react-router'
import { ZoneDetail } from '@/features/zones/zone-detail'

export const Route = createFileRoute('/_app/zones/$zone')({
  component: ZonePage,
})

function ZonePage() {
  const { zone } = Route.useParams()
  return <ZoneDetail scope="user" zone={zone} />
}
