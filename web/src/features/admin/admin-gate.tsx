import type { FormEvent, ReactNode } from 'react'
import { useState } from 'react'
import { ErrorAlert } from '@/shared/components/error-alert'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { request } from '@/shared/lib/http'
import { useSession } from '@/shared/lib/session'

/** Renders `children` once an admin key is stored for this browser session. */
export function AdminGate({ children }: { children: ReactNode }) {
  const adminKey = useSession(s => s.adminKey)
  const setAdminKey = useSession(s => s.setAdminKey)
  const [value, setValue] = useState('')
  const [error, setError] = useState<unknown>(null)
  const [busy, setBusy] = useState(false)

  if (adminKey)
    return <>{children}</>

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    setAdminKey(value.trim())
    try {
      await request('/admin/users', { admin: true })
    }
    catch (err) {
      setAdminKey(null)
      setError(err)
    }
    finally {
      setBusy(false)
    }
  }

  return (
    <Card className="max-w-md">
      <CardHeader>
        <CardTitle>Admin area</CardTitle>
        <CardDescription>Enter the server's admin key. It is kept for this browser session only.</CardDescription>
      </CardHeader>
      <CardContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <Field>
            <FieldLabel htmlFor="admin-key">Admin key</FieldLabel>
            <Input id="admin-key" type="password" autoComplete="off" required autoFocus value={value} onChange={e => setValue(e.target.value)} />
            <FieldDescription>
              The
              {' '}
              <code>api.admin_key</code>
              {' '}
              value from the server configuration.
            </FieldDescription>
          </Field>
          <ErrorAlert error={error} title="Key rejected" />
          <Button type="submit" disabled={busy} className="self-start">Unlock</Button>
        </form>
      </CardContent>
    </Card>
  )
}
