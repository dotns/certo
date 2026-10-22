import type { Scope } from './api'
import { PlusIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
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
  const { t } = useTranslation()
  const create = useCreateZoneMutation(scope)
  const [name, setName] = useState('')

  return (
    <FormDialog
      trigger={(
        <Button>
          <PlusIcon data-icon="inline-start" />
          {t('zones.add')}
        </Button>
      )}
      title={t('zones.addDialog.title')}
      description={t('zones.addDialog.description')}
      submitLabel={t('zones.addDialog.submit')}
      onSubmit={async () => {
        const zone = await create.mutateAsync({ name: name.trim().toLowerCase().replace(/\.$/, ''), ...(userId ? { user_id: userId } : {}) })
        toast.success(zone.status === 'active' ? t('zones.created.active', { zone: zone.name }) : t('zones.created.pending', { zone: zone.name }))
        onCreated?.(zone.name)
      }}
      onClose={() => setName('')}
    >
      <Field>
        <FieldLabel htmlFor="zone-name">{t('zones.addDialog.label')}</FieldLabel>
        <Input id="zone-name" placeholder="example.com" required autoFocus value={name} onChange={e => setName(e.target.value)} />
        <FieldDescription>{t('zones.addDialog.hint', { ns: nsTarget })}</FieldDescription>
      </Field>
    </FormDialog>
  )
}
