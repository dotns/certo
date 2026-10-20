import type { FormEvent, ReactNode } from 'react'
import { useState } from 'react'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Button } from '@/shared/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/shared/components/ui/dialog'

interface FormDialogProps {
  trigger: ReactNode
  title: string
  description?: ReactNode
  submitLabel?: string
  children: ReactNode
  /** Runs on submit; the dialog closes when it resolves. */
  onSubmit: () => Promise<unknown>
  /** Called whenever the dialog closes, to reset form state. */
  onClose?: () => void
  open?: boolean
  onOpenChange?: (open: boolean) => void
}

/** A dialog wrapping a form: submit runs the action, errors stay inline. */
export function FormDialog({ trigger, title, description, submitLabel = 'Save', children, onSubmit, onClose, open: controlledOpen, onOpenChange }: FormDialogProps) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false)
  const open = controlledOpen ?? uncontrolledOpen
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  const setOpen = (next: boolean) => {
    setUncontrolledOpen(next)
    onOpenChange?.(next)
    if (!next) {
      setError(null)
      onClose?.()
    }
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await onSubmit()
      setOpen(false)
    }
    catch (err) {
      setError(err)
    }
    finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={trigger as React.ReactElement} />
      <DialogContent>
        <form onSubmit={submit} className="contents">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            {description && <DialogDescription>{description}</DialogDescription>}
          </DialogHeader>
          <div className="flex flex-col gap-4">{children}</div>
          <ErrorAlert error={error} />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)} disabled={busy}>Cancel</Button>
            <Button type="submit" disabled={busy}>{submitLabel}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
