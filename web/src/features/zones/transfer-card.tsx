import type { FormEvent } from 'react'
import type { Scope, TsigKey } from './api'
import { PlusIcon, Trash2Icon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { CodeBlock } from '@/shared/components/code-block'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { Textarea } from '@/shared/components/ui/textarea'
import { useAddTsigKeyMutation, useDeleteTsigKeyMutation, useSetTransferMutation, useTransferQuery } from './hooks'
import { parseAllowList, TSIG_ALGORITHMS } from './rrsets'

export function TransferCard({ scope, zone, nsTarget }: { scope: Scope, zone: string, nsTarget: string }) {
  const { t } = useTranslation()
  const transfer = useTransferQuery(scope, zone, true)
  const save = useSetTransferMutation(scope, zone)
  const addKey = useAddTsigKeyMutation(scope, zone)
  const deleteKey = useDeleteTsigKeyMutation(scope, zone)
  const [allowFrom, setAllowFrom] = useState<string | null>(null)
  const [keyName, setKeyName] = useState('')
  const [algorithm, setAlgorithm] = useState<string>(TSIG_ALGORITHMS[0])
  const [created, setCreated] = useState<TsigKey | null>(null)
  const allowText = allowFrom ?? transfer.data?.allow_from.join('\n') ?? ''

  const submitAllow = (e: FormEvent) => {
    e.preventDefault()
    save.mutate(parseAllowList(allowText), {
      onSuccess: () => {
        setAllowFrom(null)
        toast.success(t('zones.transfer.saved'))
      },
    })
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('zones.transfer.title')}</CardTitle>
        <CardDescription>{t('zones.transfer.description')}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        <ErrorAlert error={transfer.error ?? save.error ?? deleteKey.error} />
        {transfer.isPending && <Loading />}
        {transfer.data && (
          <>
            <form onSubmit={submitAllow} className="flex flex-col gap-2">
              <Field>
                <FieldLabel htmlFor="allow-from">{t('zones.transfer.allowFrom')}</FieldLabel>
                <Textarea id="allow-from" rows={2} placeholder="192.0.2.53, 2001:db8::/64" value={allowText} onChange={e => setAllowFrom(e.target.value)} className="font-mono text-xs" />
                <FieldDescription>{t('zones.transfer.allowHint')}</FieldDescription>
              </Field>
              <Button type="submit" variant="outline" size="sm" className="self-start" disabled={save.isPending || allowFrom === null}>{t('zones.transfer.saveAddresses')}</Button>
            </form>
            <div className="flex flex-col gap-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-sm font-medium">{t('zones.transfer.tsigKeys')}</span>
                <FormDialog
                  trigger={(
                    <Button variant="outline" size="sm">
                      <PlusIcon data-icon="inline-start" />
                      {t('zones.transfer.createKey')}
                    </Button>
                  )}
                  title={t('zones.transfer.dialog.title')}
                  description={t('zones.transfer.dialog.description')}
                  submitLabel={t('zones.transfer.dialog.submit')}
                  onSubmit={async () => {
                    const key = await addKey.mutateAsync({ name: keyName.trim(), algorithm })
                    setCreated(key)
                  }}
                  onClose={() => setKeyName('')}
                >
                  <Field>
                    <FieldLabel htmlFor="tsig-name">{t('zones.transfer.dialog.name')}</FieldLabel>
                    <Input id="tsig-name" placeholder={`xfr.${zone}`} required autoFocus value={keyName} onChange={e => setKeyName(e.target.value)} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="tsig-alg">{t('zones.transfer.dialog.algorithm')}</FieldLabel>
                    <Select value={algorithm} onValueChange={v => setAlgorithm(String(v))}>
                      <SelectTrigger id="tsig-alg" className="w-44">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {TSIG_ALGORITHMS.map(a => <SelectItem key={a} value={a}>{a}</SelectItem>)}
                      </SelectContent>
                    </Select>
                  </Field>
                </FormDialog>
              </div>
              {transfer.data.tsig_keys.length === 0 && <p className="text-sm text-muted-foreground">{t('zones.transfer.noKeys')}</p>}
              {transfer.data.tsig_keys.length > 0 && (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('zones.transfer.columns.name')}</TableHead>
                      <TableHead>{t('zones.transfer.columns.algorithm')}</TableHead>
                      <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {transfer.data.tsig_keys.map(k => (
                      <TableRow key={k.name}>
                        <TableCell className="font-mono text-xs">{k.name}</TableCell>
                        <TableCell>{k.algorithm}</TableCell>
                        <TableCell>
                          <ConfirmDialog
                            trigger={<Button variant="ghost" size="icon-sm" aria-label={t('zones.transfer.delete.label', { name: k.name })}><Trash2Icon /></Button>}
                            title={t('zones.transfer.delete.title', { name: k.name })}
                            description={t('zones.transfer.delete.description')}
                            confirmLabel={t('zones.transfer.delete.confirm')}
                            onConfirm={async () => {
                              await deleteKey.mutateAsync(k.name)
                              toast.success(t('zones.transfer.delete.done', { name: k.name }))
                            }}
                          />
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </div>
          </>
        )}
      </CardContent>
      <Dialog open={created !== null} onOpenChange={open => !open && setCreated(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('zones.transfer.created.title')}</DialogTitle>
            <DialogDescription>{t('zones.transfer.created.description')}</DialogDescription>
          </DialogHeader>
          {created?.secret && (
            <div className="flex flex-col gap-3">
              <CodeBlock title={t('zones.transfer.created.secret')} code={created.secret} wrap />
              <CodeBlock title={t('zones.transfer.created.test')} code={`dig @${nsTarget} ${zone} AXFR -y ${created.algorithm}:${created.name}:${created.secret}`} wrap />
            </div>
          )}
          <DialogFooter showCloseButton />
        </DialogContent>
      </Dialog>
    </Card>
  )
}
