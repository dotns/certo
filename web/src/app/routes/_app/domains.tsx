import type { CnameCheck, Domain } from '@/features/domains/api'
import { createFileRoute } from '@tanstack/react-router'
import { ChevronDownIcon, ChevronRightIcon, PlusIcon, Trash2Icon } from 'lucide-react'
import { Fragment, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
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
function cnameLine(d: Domain): string {
  return `_acme-challenge.${d.domain}. IN CNAME ${d.cname_target}.`
}

function DomainsPage() {
  const { t } = useTranslation()
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
    document.title = t('titles.domains')
  }, [t])

  const visible = (domains.data ?? []).filter(d => matches(query, d.domain, d.cname_target))
  const tokensFor = (d: Domain) => (records.data ?? []).filter(r => r.domain === d.cname_target)

  return (
    <>
      <PageHeader
        title={t('domains.title')}
        description={t('domains.description')}
        actions={(
          <FormDialog
            trigger={(
              <Button>
                <PlusIcon data-icon="inline-start" />
                {t('domains.add')}
              </Button>
            )}
            title={t('domains.addDialog.title')}
            description={t('domains.addDialog.description')}
            submitLabel={t('domains.addDialog.submit')}
            onSubmit={async () => {
              const d = await add.mutateAsync(newDomain.trim().toLowerCase())
              setCreated(d)
            }}
            onClose={() => setNewDomain('')}
          >
            <Field>
              <FieldLabel htmlFor="new-domain">{t('domains.addDialog.label')}</FieldLabel>
              <Input id="new-domain" placeholder="example.com" required autoFocus value={newDomain} onChange={e => setNewDomain(e.target.value)} />
              <FieldDescription>{t('domains.addDialog.hint')}</FieldDescription>
            </Field>
          </FormDialog>
        )}
      />
      <ErrorAlert error={domains.error} title={t('domains.loadError')} />
      {domains.isPending && <Loading />}
      {domains.data && domains.data.length === 0 && (
        <EmptyState title={t('domains.empty.title')} description={t('domains.empty.description')} />
      )}
      {domains.data && domains.data.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>{t('domains.card.title')}</CardTitle>
            <CardDescription>{t('domains.card.description')}</CardDescription>
            <SearchInput value={query} onChange={setQuery} placeholder={t('domains.filter')} />
          </CardHeader>
          <CardContent>
            {visible.length === 0 && <p className="text-sm text-muted-foreground">{t('domains.noMatch')}</p>}
            {visible.length > 0 && (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-0"><span className="sr-only">{t('domains.columns.expand')}</span></TableHead>
                    <TableHead>{t('domains.columns.domain')}</TableHead>
                    <TableHead>{t('domains.columns.target')}</TableHead>
                    <TableHead>{t('domains.columns.status')}</TableHead>
                    <TableHead className="w-0"><span className="sr-only">{t('common.actions')}</span></TableHead>
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
                            <Button type="button" variant="ghost" size="icon-xs" aria-label={open ? t('domains.collapse', { domain: d.domain }) : t('domains.expand', { domain: d.domain })} aria-expanded={open} onClick={() => setExpanded(open ? null : d.domain)}>
                              {open ? <ChevronDownIcon /> : <ChevronRightIcon />}
                            </Button>
                          </TableCell>
                          <TableCell className="font-medium">
                            <div className="flex flex-wrap items-center gap-2">
                              {d.domain}
                              {tokens.length > 0 && <Badge variant="secondary">{t('domains.tokens', { count: tokens.length })}</Badge>}
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
                              trigger={<Button variant="ghost" size="icon-sm" aria-label={t('domains.remove.label', { domain: d.domain })}><Trash2Icon /></Button>}
                              title={t('domains.remove.title', { domain: d.domain })}
                              description={t('domains.remove.description')}
                              confirmLabel={t('domains.remove.confirm')}
                              onConfirm={async () => {
                                await remove.mutateAsync(d.domain)
                                toast.success(t('domains.remove.done', { domain: d.domain }))
                              }}
                            />
                          </TableCell>
                        </TableRow>
                        {open && (
                          <TableRow className="bg-muted/30 hover:bg-muted/30">
                            <TableCell />
                            <TableCell colSpan={4}>
                              <div className="flex flex-col gap-3 py-1">
                                <CodeBlock title={t('domains.dnsRecord')} code={cnameLine(d)} wrap />
                                <div className="flex flex-col gap-1">
                                  <span className="text-sm font-medium">{t('domains.publishedTokens')}</span>
                                  {tokens.length === 0 && <span className="text-sm text-muted-foreground">{t('domains.noTokens')}</span>}
                                  {tokens.map(tok => (
                                    <div key={tok.value} className="flex flex-wrap items-center gap-x-3 font-mono text-xs">
                                      <span className="break-all">{tok.value}</span>
                                      <span className="text-muted-foreground">{formatIso(tok.last_update)}</span>
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
          <CardTitle>{t('domains.records.title')}</CardTitle>
          <CardDescription>{t('domains.records.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <ErrorAlert error={records.error} title={t('domains.records.loadError')} />
          {records.data && records.data.length === 0 && <p className="text-sm text-muted-foreground">{t('domains.records.none')}</p>}
          {records.data && records.data.length > 0 && (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('domains.records.name')}</TableHead>
                  <TableHead>{t('domains.records.value')}</TableHead>
                  <TableHead>{t('domains.records.updated')}</TableHead>
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
            <DialogTitle>{t('domains.created.title', { domain: created?.domain ?? '' })}</DialogTitle>
            <DialogDescription>{t('domains.created.description')}</DialogDescription>
          </DialogHeader>
          {created && <CodeBlock title={t('domains.created.record')} code={cnameLine(created)} wrap />}
          <DialogFooter showCloseButton />
        </DialogContent>
      </Dialog>
    </>
  )
}
