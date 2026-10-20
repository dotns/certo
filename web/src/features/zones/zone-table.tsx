import type { Scope, Zone } from './api'
import { Link } from '@tanstack/react-router'
import { RefreshCwIcon, Trash2Icon } from 'lucide-react'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { Button } from '@/shared/components/ui/button'
import { Switch } from '@/shared/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { formatUnix } from '@/shared/lib/format'
import { useActivationMutation, useDeleteZoneMutation, useVerifyZoneMutation } from './hooks'
import { ZoneStatusBadge } from './zone-status'

interface ZoneTableProps {
  scope: Scope
  zones: Zone[]
  onError: (err: unknown) => void
}

/** Zone list shared by the user and admin views. */
export function ZoneTable({ scope, zones, onError }: ZoneTableProps) {
  const verify = useVerifyZoneMutation(scope)
  const remove = useDeleteZoneMutation(scope)
  const activation = useActivationMutation()
  const admin = scope === 'admin'

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Zone</TableHead>
          {admin && <TableHead>Owner</TableHead>}
          <TableHead>Status</TableHead>
          {admin && <TableHead>Exempt</TableHead>}
          <TableHead>Last check</TableHead>
          <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {zones.map(z => (
          <TableRow key={z.name}>
            <TableCell className="font-medium">
              <Link to={admin ? '/manage/zones/$zone' : '/zones/$zone'} params={{ zone: z.name }} className="underline-offset-4 hover:underline">
                {z.name}
              </Link>
            </TableCell>
            {admin && <TableCell className="text-muted-foreground">{z.owner ?? ''}</TableCell>}
            <TableCell>
              <div className="flex flex-col gap-1">
                <ZoneStatusBadge zone={z} />
                {z.status === 'pending' && z.last_error && <span className="text-xs text-muted-foreground">{z.last_error}</span>}
              </div>
            </TableCell>
            {admin && (
              <TableCell>
                {!z.system && (
                  <Switch
                    aria-label={`Exempt ${z.name} from the delegation check`}
                    checked={z.exempt}
                    onCheckedChange={checked => activation.mutate({ zone: z.name, exempt: checked }, { onError })}
                  />
                )}
              </TableCell>
            )}
            <TableCell className="text-muted-foreground">{formatUnix(z.last_check) || 'never'}</TableCell>
            <TableCell>
              <div className="flex items-center justify-end gap-1">
                {!z.system && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Verify delegation of ${z.name}`}
                    disabled={verify.isPending}
                    onClick={() => verify.mutate(z.name, { onError })}
                  >
                    <RefreshCwIcon />
                  </Button>
                )}
                {!z.system && (
                  <ConfirmDialog
                    trigger={<Button variant="ghost" size="icon-sm" aria-label={`Delete zone ${z.name}`}><Trash2Icon /></Button>}
                    title={`Delete ${z.name}?`}
                    description="All records, keys and transfer settings of the zone are removed and it stops resolving."
                    confirmLabel="Delete zone"
                    onConfirm={() => remove.mutateAsync(z.name)}
                  />
                )}
              </div>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
