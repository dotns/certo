import type { RrSet, Scope } from './api'
import { PencilIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { EmptyState } from '@/shared/components/empty-state'
import { Button } from '@/shared/components/ui/button'
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { usePatchRrsetsMutation } from './hooks'
import { RrsetDialog } from './rrset-dialog'
import { deleteChange, isManagedType, relativeName } from './rrsets'

interface RrsetTableProps {
  scope: Scope
  zone: string
  rrsets: RrSet[]
}

export function RrsetTable({ scope, zone, rrsets }: RrsetTableProps) {
  const patch = usePatchRrsetsMutation(scope, zone)
  return (
    <Card>
      <CardHeader>
        <CardTitle>Records</CardTitle>
        <CardDescription>Grouped by name and type; SOA is maintained by the server.</CardDescription>
        <CardAction>
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
      </CardHeader>
      <CardContent>
        {rrsets.length === 0 && <EmptyState title="No records" />}
        {rrsets.length > 0 && (
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
              {rrsets.map(rr => (
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
                          onConfirm={() => patch.mutateAsync([deleteChange(rr)])}
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
