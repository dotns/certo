import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { SearchInput } from '@/shared/components/search-input'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { matches } from '@/shared/lib/filter'
import { formatIso } from '@/shared/lib/format'
import { useAdminRecordsQuery } from './hooks'

export function AdminRecords() {
  const { t } = useTranslation()
  const records = useAdminRecordsQuery()
  const [query, setQuery] = useState('')
  const visible = (records.data ?? []).filter(r => matches(query, r.domain, r.value))
  return (
    <div className="flex flex-col gap-4 pt-2">
      <SearchInput value={query} onChange={setQuery} placeholder={t('admin.records.filter')} />
      <ErrorAlert error={records.error} title={t('admin.records.loadError')} />
      {records.isPending && <Loading />}
      {records.data && records.data.length === 0 && <EmptyState title={t('admin.records.empty')} />}
      {records.data && records.data.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">{t('admin.records.noMatch')}</p>}
      {visible.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('admin.records.name')}</TableHead>
                  <TableHead>{t('admin.records.value')}</TableHead>
                  <TableHead>{t('admin.records.updated')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {visible.map(r => (
                  <TableRow key={`${r.domain}:${r.value}`}>
                    <TableCell className="font-mono text-xs">{r.domain}</TableCell>
                    <TableCell className="font-mono text-xs break-all">{r.value}</TableCell>
                    <TableCell className="text-muted-foreground">{formatIso(r.last_update)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
