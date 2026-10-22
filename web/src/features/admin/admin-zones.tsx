import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useInfoQuery } from '@/features/info/hooks'
import { CreateZoneDialog } from '@/features/zones/create-zone-dialog'
import { useZonesQuery } from '@/features/zones/hooks'
import { ZoneTable } from '@/features/zones/zone-table'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { SearchInput } from '@/shared/components/search-input'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Label } from '@/shared/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { matches } from '@/shared/lib/filter'
import { useAdminUsersQuery } from './hooks'

export function AdminZones() {
  const { t } = useTranslation()
  const zones = useZonesQuery('admin')
  const users = useAdminUsersQuery()
  const info = useInfoQuery()
  const [owner, setOwner] = useState<string>('')
  const [actionError, setActionError] = useState<unknown>(null)
  const [query, setQuery] = useState('')
  const visible = (zones.data ?? []).filter(z => matches(query, z.name, z.owner, z.status))

  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex flex-col gap-2">
          <p className="text-sm text-muted-foreground">{t('admin.zones.hint')}</p>
          <SearchInput value={query} onChange={setQuery} placeholder={t('admin.zones.filter')} />
        </div>
        <div className="flex items-end gap-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="zone-owner">{t('admin.zones.createFor')}</Label>
            <Select value={owner} items={(users.data ?? []).map(u => ({ value: String(u.id), label: u.username }))} onValueChange={v => setOwner(String(v ?? ''))}>
              <SelectTrigger id="zone-owner" className="w-44">
                <SelectValue placeholder={t('admin.zones.chooseUser')} />
              </SelectTrigger>
              <SelectContent>
                {(users.data ?? []).map(u => <SelectItem key={u.id} value={String(u.id)}>{u.username}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          {owner && <CreateZoneDialog scope="admin" nsTarget={info.data?.ns_target ?? ''} userId={Number(owner)} />}
        </div>
      </div>
      <ErrorAlert error={zones.error} title={t('admin.zones.loadError')} />
      <ErrorAlert error={actionError} />
      {zones.isPending && <Loading />}
      {zones.data && zones.data.length === 0 && <EmptyState title={t('admin.zones.empty')} />}
      {zones.data && zones.data.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">{t('admin.zones.noMatch')}</p>}
      {visible.length > 0 && (
        <Card>
          <CardContent>
            <ZoneTable scope="admin" zones={visible} onError={setActionError} />
          </CardContent>
        </Card>
      )}
    </div>
  )
}
