import { createFileRoute } from '@tanstack/react-router'
import { PlusIcon, Trash2Icon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useAddDomainMutation, useDomainsQuery, useRecordsQuery, useRemoveDomainMutation } from '@/features/domains/hooks'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { CopyButton } from '@/shared/components/copy-button'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { formatIso } from '@/shared/lib/format'

export const Route = createFileRoute('/_app/domains')({
  component: DomainsPage,
})

function DomainsPage() {
  const domains = useDomainsQuery()
  const records = useRecordsQuery()
  const add = useAddDomainMutation()
  const remove = useRemoveDomainMutation()
  const [newDomain, setNewDomain] = useState('')

  useEffect(() => {
    document.title = 'Domains · certo'
  }, [])

  return (
    <>
      <PageHeader
        title="Challenge domains"
        description="Each domain gets a CNAME target; ACME clients write their DNS-01 tokens there."
        actions={(
          <FormDialog
            trigger={(
              <Button>
                <PlusIcon data-icon="inline-start" />
                Add domain
              </Button>
            )}
            title="Add domain"
            description="The domain you request certificates for. A CNAME from its _acme-challenge name to the target shown afterwards completes the setup."
            submitLabel="Add domain"
            onSubmit={() => add.mutateAsync(newDomain.trim().toLowerCase())}
            onClose={() => setNewDomain('')}
          >
            <Field>
              <FieldLabel htmlFor="new-domain">Domain</FieldLabel>
              <Input id="new-domain" placeholder="example.com" required autoFocus value={newDomain} onChange={e => setNewDomain(e.target.value)} />
              <FieldDescription>Wildcards and subdomains of this name use the same CNAME.</FieldDescription>
            </Field>
          </FormDialog>
        )}
      />
      <ErrorAlert error={domains.error} title="Domains could not be loaded" />
      {domains.isPending && <Loading />}
      {domains.data && domains.data.length === 0 && (
        <EmptyState title="No domains yet" description="Add a domain to get its CNAME target, or let a global API key create it on first use." />
      )}
      {domains.data && domains.data.length > 0 && (
        <Card>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Domain</TableHead>
                  <TableHead>CNAME name</TableHead>
                  <TableHead>CNAME target</TableHead>
                  <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {domains.data.map((d) => {
                  const name = `_acme-challenge.${d.domain}`
                  return (
                    <TableRow key={d.domain}>
                      <TableCell className="font-medium">{d.domain}</TableCell>
                      <TableCell>
                        <span className="inline-flex items-center gap-1 font-mono text-xs">
                          {name}
                          <CopyButton text={name} label="" />
                        </span>
                      </TableCell>
                      <TableCell>
                        <span className="inline-flex items-center gap-1 font-mono text-xs">
                          {d.cname_target}
                          <CopyButton text={d.cname_target} label="" />
                        </span>
                      </TableCell>
                      <TableCell>
                        <ConfirmDialog
                          trigger={<Button variant="ghost" size="icon-sm" aria-label={`Remove ${d.domain}`}><Trash2Icon /></Button>}
                          title={`Remove ${d.domain}?`}
                          description="Its challenge records are deleted and clients using the CNAME stop working."
                          confirmLabel="Remove"
                          onConfirm={() => remove.mutateAsync(d.domain)}
                        />
                      </TableCell>
                    </TableRow>
                  )
                })}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
      <Card>
        <CardHeader>
          <CardTitle>Active TXT records</CardTitle>
          <CardDescription>Tokens currently published for your domains; they refresh every 15 seconds.</CardDescription>
        </CardHeader>
        <CardContent>
          <ErrorAlert error={records.error} title="Records could not be loaded" />
          {records.data && records.data.length === 0 && <p className="text-sm text-muted-foreground">No tokens published right now.</p>}
          {records.data && records.data.length > 0 && (
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
          )}
        </CardContent>
      </Card>
    </>
  )
}
