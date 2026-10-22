import type { CnameCheck, Domain } from '@/features/domains/api'
import { createFileRoute } from '@tanstack/react-router'
import { ChevronDownIcon, ChevronRightIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { Fragment, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { CnameCheckButton, CnameResult } from '@/features/domains/cname-check'
import { useAddDomainMutation, useDomainsQuery, useRecordsQuery, useRemoveDomainMutation } from '@/features/domains/hooks'
import { CodeBlock } from '@/shared/components/code-block'
import { ConfirmDialog } from '@/shared/components/confirm-dialog'
import { CopyButton } from '@/shared/components/copy-button'
import { EmptyState } from '@/shared/components/empty-state'
import { ErrorAlert } from '@/shared/components/error-alert'
import { FormDialog } from '@/shared/components/form-dialog'
import { Loading } from '@/shared/components/loading'
import { PageHeader } from '@/shared/components/page-header'
import { SearchInput } from '@/shared/components/search-input'
import { Badge } from '@/shared/components/ui/badge'
import { Button } from '@/shared/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/shared/components/ui/card'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/shared/components/ui/dialog'
import { Field, FieldDescription, FieldLabel } from '@/shared/components/ui/field'
import { Input } from '@/shared/components/ui/input'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/components/ui/table'
import { matches } from '@/shared/lib/filter'
import { formatIso } from '@/shared/lib/format'

export const Route = createFileRoute('/_app/domains')({
  component: DomainsPage,
})

/** The zone-file line a user adds at their DNS provider. */
export function cnameLine(d: Domain): string {
  return `_acme-challenge.${d.domain}. IN CNAME ${d.cname_target}.`
}

function DomainsPage() {
  const domains = useDomainsQuery()
  const records = useRecordsQuery()
  const add = useAddDomainMutation()
  const remove = useRemoveDomainMutation()
  const [newDomain, setNewDomain] = useState('')
  const [created, setCreated] = useState<Domain | null>(null)
  const [query, setQuery] = useState('')
  const [expanded, setExpanded] = useState<string | null>(null)
  const [checks, setChecks] = useState<Record<string, CnameCheck>>({})

  useEffect(() => {
    document.title = 'Domains · certo'
  }, [])

  const visible = (domains.data ?? []).filter(d => matches(query, d.domain, d.cname_target))
  const tokensFor = (d: Domain) => (records.data ?? []).filter(r => r.domain === d.cname_target)

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
            onSubmit={async () => {
              const d = await add.mutateAsync(newDomain.trim().toLowerCase())
              setCreated(d)
            }}
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
          <CardHeader>
            <CardTitle>Domains</CardTitle>
            <CardDescription>Expand a row to see its published tokens; check the CNAME after setting it up.</CardDescription>
            <SearchInput value={query} onChange={setQuery} placeholder="Filter domains…" />
          </CardHeader>
          <CardContent>
            {visible.length === 0 && <p className="text-sm text-muted-foreground">No domain matches the filter.</p>}
            {visible.length > 0 && (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-0"><span className="sr-only">Expand</span></TableHead>
                    <TableHead>Domain</TableHead>
                    <TableHead>CNAME target</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="w-0"><span className="sr-only">Actions</span></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {visible.map((d) => {
                    const open = expanded === d.domain
                    const tokens = tokensFor(d)
                    const check = checks[d.domain]
                    return (
                      <Fragment key={d.domain}>
                        <TableRow>
                          <TableCell>
                            <Button type="button" variant="ghost" size="icon-xs" aria-label={open ? `Collapse ${d.domain}` : `Expand ${d.domain}`} aria-expanded={open} onClick={() => setExpanded(open ? null : d.domain)}>
                              {open ? <ChevronDownIcon /> : <ChevronRightIcon />}
                            </Button>
                          </TableCell>
                          <TableCell className="font-medium">
                            <div className="flex flex-wrap items-center gap-2">
                              {d.domain}
                              {tokens.length > 0 && <Badge variant="secondary">{tokens.length === 1 ? '1 token' : `${tokens.length} tokens`}</Badge>}
                            </div>
                          </TableCell>
                          <TableCell>
                            <span className="inline-flex items-center gap-1 font-mono text-xs">
                              {d.cname_target}
                              <CopyButton text={d.cname_target} label="" />
                            </span>
                          </TableCell>
                          <TableCell>
                            <div className="flex flex-wrap items-center gap-2">
                              <CnameCheckButton domain={d.domain} onResult={r => setChecks(c => ({ ...c, [d.domain]: r }))} />
                              {check && <CnameResult result={check} />}
                            </div>
                          </TableCell>
                          <TableCell>
                            <ConfirmDialog
                              trigger={<Button variant="ghost" size="icon-sm" aria-label={`Remove ${d.domain}`}><Trash2Icon /></Button>}
                              title={`Remove ${d.domain}?`}
                              description="Its challenge records are deleted and clients using the CNAME stop working."
                              confirmLabel="Remove"
                              onConfirm={async () => {
                                await remove.mutateAsync(d.domain)
                                toast.success(`${d.domain} removed`)
                              }}
                            />
                          </TableCell>
                        </TableRow>
                        {open && (
                          <TableRow className="bg-muted/30 hover:bg-muted/30">
                            <TableCell />
                            <TableCell colSpan={4}>
                              <div className="flex flex-col gap-3 py-1">
                                <CodeBlock title="DNS record to add at your provider" code={cnameLine(d)} wrap />
                                <div className="flex flex-col gap-1">
                                  <span className="text-sm font-medium">Published tokens</span>
                                  {tokens.length === 0 && <span className="text-sm text-muted-foreground">None right now; they appear while a client validates a challenge.</span>}
                                  {tokens.map(t => (
                                    <div key={t.value} className="flex flex-wrap items-center gap-x-3 font-mono text-xs">
                                      <span className="break-all">{t.value}</span>
                                      <span className="text-muted-foreground">{formatIso(t.last_update)}</span>
                                    </div>
                                  ))}
                                </div>
                              </div>
                            </TableCell>
                          </TableRow>
                        )}
                      </Fragment>
                    )
                  })}
                </TableBody>
              </Table>
            )}
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
      <Dialog open={created !== null} onOpenChange={open => !open && setCreated(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{`${created?.domain ?? ''} added`}</DialogTitle>
            <DialogDescription>Create this record at your DNS provider, then use "Check CNAME" on the domain row.</DialogDescription>
          </DialogHeader>
          {created && <CodeBlock title="DNS record" code={cnameLine(created)} wrap />}
          <DialogFooter showCloseButton />
        </DialogContent>
      </Dialog>
    </>
  )
}
