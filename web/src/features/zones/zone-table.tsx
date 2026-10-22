import type { Scope, Zone } from './api'
import { Link } from '@tanstack/react-router'
import { RefreshCwIcon, Trash2Icon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
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
  const { t } = useTranslation()
  const verify = useVerifyZoneMutation(scope)
  const remove = useDeleteZoneMutation(scope)
  const activation = useActivationMutation()
  const admin = scope === 'admin'

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>{t('zones.columns.zone')}</TableHead>
          {admin && <TableHead>{t('zones.columns.owner')}</TableHead>}
          <TableHead>{t('zones.columns.status')}</TableHead>
          {admin && <TableHead>{t('zones.columns.exempt')}</TableHead>}
          <TableHead>{t('zones.columns.lastCheck')}</TableHead>
          <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
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
                    aria-label={t('zones.exemptLabel', { zone: z.name })}
                    checked={z.exempt}
                    onCheckedChange={checked => activation.mutate({ zone: z.name, exempt: checked }, { onError, onSuccess: () => toast.success(checked ? t('zones.exempted', { zone: z.name }) : t('zones.unexempted', { zone: z.name })) })}
                  />
                )}
              </TableCell>
            )}
            <TableCell className="text-muted-foreground">{formatUnix(z.last_check) || t('common.never')}</TableCell>
            <TableCell>
              <div className="flex items-center justify-end gap-1">
                {!z.system && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    aria-label={t('zones.verifyLabel', { zone: z.name })}
                    disabled={verify.isPending}
                    onClick={() => verify.mutate(z.name, { onError, onSuccess: v => toast.success(v.status === 'active' ? t('zones.verified.active', { zone: z.name }) : t('zones.verified.pending', { zone: z.name })) })}
                  >
                    <RefreshCwIcon />
                  </Button>
                )}
                {!z.system && (
                  <ConfirmDialog
                    trigger={<Button variant="ghost" size="icon-sm" aria-label={t('zones.delete.label', { zone: z.name })}><Trash2Icon /></Button>}
                    title={t('zones.delete.title', { zone: z.name })}
                    description={t('zones.delete.description')}
                    confirmLabel={t('zones.delete.confirm')}
                    onConfirm={async () => {
                      await remove.mutateAsync(z.name)
                      toast.success(t('zones.delete.done', { zone: z.name }))
                    }}
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
