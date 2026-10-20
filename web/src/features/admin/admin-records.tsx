import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Loading } from '@/shared/components/loading'
import { Card, CardContent } from '@/shared/components/ui/card'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { formatIso } from '@/shared/lib/format'
import { useAdminRecordsQuery } from './hooks'

export function AdminRecords() {
  const records = useAdminRecordsQuery()
  return (
    <div className="flex flex-col gap-4 pt-2">
      <ErrorAlert error={records.error} title="Records could not be loaded" />
      {records.isPending && <Loading />}
      {records.data && records.data.length === 0 && <EmptyState title="No tokens published" />}
      {records.data && records.data.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Value</TableHead>
                  <TableHead>Updated</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {records.data.map(r => (
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
