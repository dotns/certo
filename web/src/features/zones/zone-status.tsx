import type { Zone } from './api'
import { useTranslation } from 'react-i18next'
import { Badge } from '@/shared/components/ui/badge'
import { zoneTone } from './rrsets'

const VARIANTS = { pending: 'outline', active: 'default', exempt: 'secondary' } as const

export function ZoneStatusBadge({ zone }: { zone: Pick<Zone, 'status' | 'exempt' | 'system'> }) {
  const { t } = useTranslation()
  const tone = zoneTone(zone)
  return <Badge variant={VARIANTS[tone]}>{zone.system ? t('zones.status.base') : t(`zones.status.${tone}`)}</Badge>
}
