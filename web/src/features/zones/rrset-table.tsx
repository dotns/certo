import type { RrSet, Scope } from './api'
import { DownloadIcon, PencilIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { SearchInput } from '@/shared/components/search-input'
import { Button } from '@/shared/components/ui/button'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { downloadText } from '@/shared/lib/download'
import { matches } from '@/shared/lib/filter'
import { usePatchRrsetsMutation } from './hooks'
import { ImportDialog } from './import-dialog'
import { RrsetDialog } from './rrset-dialog'
import { deleteChange, isManagedType, relativeName } from './rrsets'
import { exportZoneFile } from './zonefile'

interface RrsetTableProps {
  scope: Scope
  zone: string
  rrsets: RrSet[]
}

export function RrsetTable({ scope, zone, rrsets }: RrsetTableProps) {
  const { t } = useTranslation()
  const patch = usePatchRrsetsMutation(scope, zone)
  const [query, setQuery] = useState('')
  const [type, setType] = useState<string | null>(null)
  const types = [...new Set(rrsets.map(rr => rr.type))].sort()
  const visible = rrsets.filter(rr => (type === null || rr.type === type) && matches(query, relativeName(rr.name, zone), rr.type, ...rr.records))

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('zones.records.title')}</CardTitle>
        <CardDescription>{t('zones.records.description')}</CardDescription>
        <CardAction className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm" disabled={rrsets.length === 0} onClick={() => downloadText(`${zone}.zone`, exportZoneFile(zone, rrsets))}>
            <DownloadIcon data-icon="inline-start" />
            {t('zones.records.export')}
          </Button>
          <ImportDialog scope={scope} zone={zone} />
          <RrsetDialog
            scope={scope}
            zone={zone}
            trigger={(
              <Button size="sm">
                <PlusIcon data-icon="inline-start" />
                {t('zones.records.add')}
              </Button>
            )}
          />
        </CardAction>
        {rrsets.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 pt-2">
            <SearchInput value={query} onChange={setQuery} placeholder={t('zones.records.filter')} />
            <div className="flex flex-wrap items-center gap-1" role="group" aria-label={t('zones.records.typeGroup')}>
              <Button type="button" variant={type === null ? 'secondary' : 'ghost'} size="xs" aria-pressed={type === null} onClick={() => setType(null)}>{t('zones.records.all')}</Button>
              {types.map(rt => (
                <Button key={rt} type="button" variant={type === rt ? 'secondary' : 'ghost'} size="xs" aria-pressed={type === rt} onClick={() => setType(type === rt ? null : rt)}>{rt}</Button>
              ))}
            </div>
          </div>
        )}
      </CardHeader>
      <CardContent>
        {rrsets.length === 0 && <EmptyState title={t('zones.records.empty.title')} description={t('zones.records.empty.description')} />}
        {rrsets.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">{t('zones.records.noMatch')}</p>}
        {visible.length > 0 && (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('zones.records.columns.name')}</TableHead>
                <TableHead>{t('zones.records.columns.type')}</TableHead>
                <TableHead>{t('zones.records.columns.ttl')}</TableHead>
                <TableHead>{t('zones.records.columns.records')}</TableHead>
                <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {visible.map(rr => (
                <TableRow key={`${rr.name}/${rr.type}`}>
                  <TableCell className="font-mono text-xs">{relativeName(rr.name, zone)}</TableCell>
                  <TableCell className="font-medium">{rr.type}</TableCell>
                  <TableCell className="text-muted-foreground">{rr.ttl}</TableCell>
                  <TableCell>
                    <ul className="flex flex-col gap-0.5 font-mono text-xs break-all">
                      {rr.records.map(r => <li key={r}>{r}</li>)}
                    </ul>
                  </TableCell>
                  <TableCell>
                    {isManagedType(rr.type) && (
                      <div className="flex items-center justify-end gap-1">
                        <RrsetDialog
                          scope={scope}
                          zone={zone}
                          rrset={rr}
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={t('zones.records.edit', { type: rr.type, name: rr.name })}><PencilIcon /></Button>}
                        />
                        <ConfirmDialog
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={t('zones.records.deleteLabel', { type: rr.type, name: rr.name })}><Trash2Icon /></Button>}
                          title={t('zones.records.deleteTitle', { type: rr.type, name: rr.name })}
                          description={t('zones.records.deleteDescription')}
                          confirmLabel={t('zones.records.deleteConfirm')}
                          onConfirm={async () => {
                            await patch.mutateAsync([deleteChange(rr)])
                            toast.success(t('zones.records.deleted', { type: rr.type, name: relativeName(rr.name, zone) }))
                          }}
                        />
                      </div>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  )
}
