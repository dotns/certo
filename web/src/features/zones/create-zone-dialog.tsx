import type { Scope } from './api'
import { PlusIcon } from 'lucide-react'
import { useState } from 'react'
import { FormDialog } from '@/shared/components/form-dialog'
import { Button } from '@/shared/components/ui/button'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { useCreateZoneMutation } from './hooks'

interface CreateZoneDialogProps {
  scope: Scope
  nsTarget: string
  /** Admin view: the owner's user id. */
  userId?: number
  onCreated?: (name: string) => void
}

export function CreateZoneDialog({ scope, nsTarget, userId, onCreated }: CreateZoneDialogProps) {
  const create = useCreateZoneMutation(scope)
  const [name, setName] = useState('')

  return (
    <FormDialog
      trigger={(
        <Button>
          <PlusIcon data-icon="inline-start" />
          Add zone
        </Button>
      )}
      title="Add zone"
      description="A forward zone (example.com) or a reverse zone (23.251.162.in-addr.arpa)."
      submitLabel="Add zone"
      onSubmit={async () => {
        const zone = await create.mutateAsync({ name: name.trim().toLowerCase().replace(/\.$/, ''), ...(userId ? { user_id: userId } : {}) })
        onCreated?.(zone.name)
      }}
      onClose={() => setName('')}
    >
      <Field>
        <FieldLabel htmlFor="zone-name">Zone name</FieldLabel>
        <Input id="zone-name" placeholder="example.com" required autoFocus value={name} onChange={e => setName(e.target.value)} />
        <FieldDescription>
          The zone is served once its parent delegates it with an NS record to
          {' '}
          <code>{nsTarget}</code>
          . Checks run hourly; you can also trigger one.
        </FieldDescription>
      </Field>
    </FormDialog>
  )
}
