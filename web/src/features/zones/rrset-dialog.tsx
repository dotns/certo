import type { ReactNode } from 'react'
import type { RrSet, Scope } from './api'
import type { RrsetForm } from './rrsets'
import { useState } from 'react'
import { toast } from 'sonner'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/shared/components/ui/select'
import { Textarea } from '@/shared/components/ui/textarea'
import { usePatchRrsetsMutation } from './hooks'
import { changeFromForm, DEFAULT_TTL, emptyForm, formFromRrset, RECORD_EXAMPLES, RECORD_TYPES, validateForm } from './rrsets'

interface RrsetDialogProps {
  scope: Scope
  zone: string
  /** Existing rrset to edit; omitted when adding. */
  rrset?: RrSet
  trigger: ReactNode
}

/** Adds or replaces one rrset (all records of a name and type). */
export function RrsetDialog({ scope, zone, rrset, trigger }: RrsetDialogProps) {
  const patch = usePatchRrsetsMutation(scope, zone)
  const initial = () => (rrset ? formFromRrset(rrset, zone) : emptyForm(zone))
  const [form, setForm] = useState<RrsetForm>(initial)
  const [errors, setErrors] = useState<string[]>([])
  const editing = rrset !== undefined

  const update = (patch: Partial<RrsetForm>) => setForm(f => ({ ...f, ...patch }))

  return (
    <FormDialog
      trigger={trigger}
      title={editing ? `Edit ${rrset.type} records of ${rrset.name}` : 'Add records'}
      description={editing ? 'All records of this name and type are replaced.' : 'One record per line; the same name and type form one set.'}
      submitLabel={editing ? 'Save' : 'Add'}
      onSubmit={async () => {
        const problems = validateForm(form)
        setErrors(problems)
        if (problems.length > 0)
          throw new Error(problems.join('. '))
        await patch.mutateAsync([changeFromForm(form)])
        toast.success(editing ? `${rrset.type} records of ${rrset.name} saved` : 'Records added')
      }}
      onClose={() => {
        setForm(initial())
        setErrors([])
      }}
    >
      <div className="grid grid-cols-[1fr_auto] gap-3">
        <Field>
          <FieldLabel htmlFor="rr-name">Name</FieldLabel>
          <Input id="rr-name" placeholder="@" value={form.name} disabled={editing} onChange={e => update({ name: e.target.value })} />
          <FieldDescription>
            Relative to
            {' '}
            <code>{zone}</code>
            ;
            {' '}
            <code>@</code>
            {' '}
            is the zone itself.
          </FieldDescription>
        </Field>
        <Field>
          <FieldLabel htmlFor="rr-type">Type</FieldLabel>
          <Select value={form.type} onValueChange={v => update({ type: String(v) })} disabled={editing}>
            <SelectTrigger id="rr-type" className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {RECORD_TYPES.map(t => <SelectItem key={t} value={t}>{t}</SelectItem>)}
            </SelectContent>
          </Select>
        </Field>
      </div>
      <Field>
        <FieldLabel htmlFor="rr-ttl">TTL (seconds)</FieldLabel>
        <Input id="rr-ttl" inputMode="numeric" placeholder={String(DEFAULT_TTL)} value={form.ttl} onChange={e => update({ ttl: e.target.value })} />
      </Field>
      <Field>
        <FieldLabel htmlFor="rr-records">Records</FieldLabel>
        <Textarea id="rr-records" rows={3} placeholder={RECORD_EXAMPLES[form.type]} value={form.records} onChange={e => update({ records: e.target.value })} className="font-mono text-xs" />
        <FieldDescription>
          {form.type === 'TXT' ? 'Plain text; quotes are added for you.' : 'Names in record data must be absolute (with a trailing dot).'}
        </FieldDescription>
      </Field>
      {errors.length > 0 && <ErrorAlert error={new Error(errors.join('. '))} title="Check the form" />}
    </FormDialog>
  )
}
