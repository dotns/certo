import type { Scope } from './api'
import type { ParsedZoneFile } from './zonefile'
import { UploadIcon } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Button } from '@/shared/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/shared/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { Textarea } from '@/shared/components/ui/textarea'
import { usePatchRrsetsMutation } from './hooks'
import { parseZoneFile } from './zonefile'

interface ImportDialogProps {
  scope: Scope
  zone: string
}

/** Pastes zone-file lines, previews the resulting changes and applies them in one PATCH. */
export function ImportDialog({ scope, zone }: ImportDialogProps) {
  const patch = usePatchRrsetsMutation(scope, zone)
  const [open, setOpen] = useState(false)
  const [text, setText] = useState('')
  const [parsed, setParsed] = useState<ParsedZoneFile | null>(null)

  const close = (next: boolean) => {
    setOpen(next)
    if (!next) {
      setText('')
      setParsed(null)
      patch.reset()
    }
  }

  const apply = () => {
    if (!parsed)
      return
    patch.mutate(parsed.changes, {
      onSuccess: () => {
        toast.success(`${parsed.changes.length} record set${parsed.changes.length === 1 ? '' : 's'} imported`)
        close(false)
      },
    })
  }

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogTrigger render={<Button type="button" variant="outline" size="sm" />}>
        <UploadIcon data-icon="inline-start" />
        Import
      </DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Import records</DialogTitle>
          <DialogDescription>Paste zone-file lines. Records of the same name and type replace the existing set; nothing else is deleted.</DialogDescription>
        </DialogHeader>
        {!parsed && (
          <Field>
            <FieldLabel htmlFor="import-text">Zone file</FieldLabel>
            <Textarea id="import-text" rows={10} className="font-mono text-xs" placeholder={`$ORIGIN ${zone}.\n@    IN A     192.0.2.1\nwww  IN CNAME ${zone}.`} value={text} onChange={e => setText(e.target.value)} />
            <FieldDescription>
              <code>$ORIGIN</code>
              {' '}
              and
              {' '}
              <code>$TTL</code>
              {' '}
              are honoured; names are relative to the zone. SOA lines are ignored.
            </FieldDescription>
          </Field>
        )}
        {parsed && (
          <div className="flex max-h-[50vh] flex-col gap-3 overflow-y-auto">
            {parsed.changes.length === 0 && <p className="text-sm text-muted-foreground">No importable records found.</p>}
            {parsed.changes.length > 0 && (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Name</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>TTL</TableHead>
                    <TableHead>Records</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {parsed.changes.map(c => (
                    <TableRow key={`${c.name}/${c.type}`}>
                      <TableCell className="font-mono text-xs">{c.name}</TableCell>
                      <TableCell className="font-medium">{c.type}</TableCell>
                      <TableCell className="text-muted-foreground">{c.ttl ?? 'default'}</TableCell>
                      <TableCell>
                        <ul className="flex flex-col gap-0.5 font-mono text-xs break-all">
                          {c.records?.map(r => <li key={r}>{r}</li>)}
                        </ul>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
            {parsed.skipped.length > 0 && (
              <div className="flex flex-col gap-1 text-sm">
                <span className="font-medium">Skipped lines</span>
                <ul className="flex flex-col gap-0.5 text-xs text-muted-foreground">
                  {parsed.skipped.map(s => (
                    <li key={s.line}>
                      {`Line ${s.line}: ${s.reason}`}
                      {' '}
                      <code className="break-all">{s.text}</code>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </div>
        )}
        <ErrorAlert error={patch.error} title="Import failed" />
        <DialogFooter>
          {!parsed && (
            <>
              <Button type="button" variant="outline" onClick={() => close(false)}>Cancel</Button>
              <Button type="button" disabled={!text.trim()} onClick={() => setParsed(parseZoneFile(text, zone))}>Preview</Button>
            </>
          )}
          {parsed && (
            <>
              <Button type="button" variant="outline" disabled={patch.isPending} onClick={() => setParsed(null)}>Back</Button>
              <Button type="button" disabled={patch.isPending || parsed.changes.length === 0} onClick={apply}>
                {`Import ${parsed.changes.length} set${parsed.changes.length === 1 ? '' : 's'}`}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
