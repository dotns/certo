import type { RrSet, Scope } from './api'
import { DownloadIcon, PencilIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
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
  const patch = usePatchRrsetsMutation(scope, zone)
  const [query, setQuery] = useState('')
  const [type, setType] = useState<string | null>(null)
  const types = [...new Set(rrsets.map(rr => rr.type))].sort()
  const visible = rrsets.filter(rr => (type === null || rr.type === type) && matches(query, relativeName(rr.name, zone), rr.type, ...rr.records))

  return (
    <Card>
      <CardHeader>
        <CardTitle>Records</CardTitle>
        <CardDescription>Grouped by name and type; SOA is maintained by the server.</CardDescription>
        <CardAction className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" size="sm" disabled={rrsets.length === 0} onClick={() => downloadText(`${zone}.zone`, exportZoneFile(zone, rrsets))}>
            <DownloadIcon data-icon="inline-start" />
            Export
          </Button>
          <ImportDialog scope={scope} zone={zone} />
          <RrsetDialog
            scope={scope}
            zone={zone}
            trigger={(
              <Button size="sm">
                <PlusIcon data-icon="inline-start" />
                Add records
              </Button>
            )}
          />
        </CardAction>
        {rrsets.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 pt-2">
            <SearchInput value={query} onChange={setQuery} placeholder="Filter by name, type or content…" />
            <div className="flex flex-wrap items-center gap-1" role="group" aria-label="Record type">
              <Button type="button" variant={type === null ? 'secondary' : 'ghost'} size="xs" aria-pressed={type === null} onClick={() => setType(null)}>All</Button>
              {types.map(t => (
                <Button key={t} type="button" variant={type === t ? 'secondary' : 'ghost'} size="xs" aria-pressed={type === t} onClick={() => setType(type === t ? null : t)}>{t}</Button>
              ))}
            </div>
          </div>
        )}
      </CardHeader>
      <CardContent>
        {rrsets.length === 0 && <EmptyState title="No records" description="Add records one set at a time or import a zone file." />}
        {rrsets.length > 0 && visible.length === 0 && <p className="text-sm text-muted-foreground">No record matches the filter.</p>}
        {visible.length > 0 && (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Type</TableHead>
                <TableHead>TTL</TableHead>
                <TableHead>Records</TableHead>
                <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
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
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={`Edit ${rr.type} records of ${rr.name}`}><PencilIcon /></Button>}
                        />
                        <ConfirmDialog
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={`Delete ${rr.type} records of ${rr.name}`}><Trash2Icon /></Button>}
                          title={`Delete ${rr.type} records of ${rr.name}?`}
                          description="All records of this name and type are removed."
                          confirmLabel="Delete"
                          onConfirm={async () => {
                            await patch.mutateAsync([deleteChange(rr)])
                            toast.success(`${rr.type} records of ${relativeName(rr.name, zone)} deleted`)
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
