import type { Zone } from './api'
import { Badge } from '@/shared/components/ui/badge'
import { zoneTone } from './rrsets'

const LABELS = { pending: 'Pending', active: 'Active', exempt: 'Active · exempt' } as const
const VARIANTS = { pending: 'outline', active: 'default', exempt: 'secondary' } as const

export function ZoneStatusBadge({ zone }: { zone: Pick<Zone, 'status' | 'exempt' | 'system'> }) {
  const tone = zoneTone(zone)
  return <Badge variant={VARIANTS[tone]}>{zone.system ? 'Base zone' : LABELS[tone]}</Badge>
}
