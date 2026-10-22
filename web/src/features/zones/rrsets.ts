import type { RrSet, RrSetChange, Zone } from './api'

/** Record types users may manage, in menu order. */
export const RECORD_TYPES = ['A', 'AAAA', 'CNAME', 'MX', 'TXT', 'NS', 'PTR', 'SRV', 'CAA'] as const

export const RECORD_EXAMPLES: Record<string, string> = {
  A: '192.0.2.1',
  AAAA: '2001:db8::1',
  CNAME: 'target.example.com.',
  MX: '10 mail.example.com.',
  TXT: 'v=spf1 -all',
  NS: 'ns1.example.net.',
  PTR: 'host.example.com.',
  SRV: '10 5 443 target.example.com.',
  CAA: '0 issue "letsencrypt.org"',
}

export const TSIG_ALGORITHMS = ['hmac-sha256', 'hmac-sha384', 'hmac-sha512'] as const
export const NSEC_MODES = ['nsec', 'nsec3'] as const
export const DEFAULT_TTL = 3600

/** Whether the zone is under `in-addr.arpa` or `ip6.arpa`. */
export function isReverseZone(zone: string): boolean {
  return /\.(?:in-addr|ip6)\.arpa$/.test(zone)
}

/** The record type a new rrset defaults to. */
export function defaultRecordType(zone: string): string {
  return isReverseZone(zone) ? 'PTR' : 'A'
}

/** `www.example.com.` → `www`; the apex → `@`. */
export function relativeName(owner: string, zone: string): string {
  const name = owner.replace(/\.$/, '')
  if (name === zone)
    return '@'
  const suffix = `.${zone}`
  return name.endsWith(suffix) ? name.slice(0, -suffix.length) : name
}

/** Types shown read-only in the dashboard. */
export function isManagedType(type: string): boolean {
  return (RECORD_TYPES as readonly string[]).includes(type)
}

/** Splits a textarea into record contents, one per line. */
export function parseRecords(text: string): string[] {
  return text.split('\n').map(l => l.trim()).filter(Boolean)
}

export interface RrsetForm {
  name: string
  type: string
  ttl: string
  records: string
}

export function emptyForm(zone: string): RrsetForm {
  return { name: '', type: defaultRecordType(zone), ttl: '', records: '' }
}

export function formFromRrset(rr: RrSet, zone: string): RrsetForm {
  return {
    name: relativeName(rr.name, zone),
    type: rr.type,
    ttl: String(rr.ttl),
    records: rr.records.map(r => (rr.type === 'TXT' ? unquoteTxt(r) : r)).join('\n'),
  }
}

/** Translation keys of the rrset form's validation errors; empty when the form can be submitted. */
export function validateForm(form: RrsetForm): string[] {
  const errors: string[] = []
  if (!form.type)
    errors.push('zones.rrset.chooseType')
  if (parseRecords(form.records).length === 0)
    errors.push('zones.rrset.enterRecord')
  if (form.ttl.trim() && !/^\d+$/.test(form.ttl.trim()))
    errors.push('zones.rrset.ttlNumber')
  return errors
}

/** The REPLACE change for a submitted form. */
export function changeFromForm(form: RrsetForm): RrSetChange {
  const ttl = form.ttl.trim()
  return {
    name: form.name.trim() || '@',
    type: form.type,
    ...(ttl ? { ttl: Number(ttl) } : {}),
    changetype: 'REPLACE',
    records: parseRecords(form.records),
  }
}

/** The DELETE change for an existing rrset. */
export function deleteChange(rr: RrSet): RrSetChange {
  return { name: rr.name, type: rr.type, changetype: 'DELETE' }
}

/** `"v=spf1 -all"` → `v=spf1 -all`; multi-string values keep their quotes. */
export function unquoteTxt(content: string): string {
  const m = content.match(/^"((?:[^"\\]|\\.)*)"$/)
  return m?.[1] !== undefined ? m[1].replace(/\\(.)/g, '$1') : content
}

export type StatusTone = 'pending' | 'active' | 'exempt'

export function zoneTone(zone: Pick<Zone, 'status' | 'exempt' | 'system'>): StatusTone {
  if (zone.status !== 'active')
    return 'pending'
  return zone.exempt || zone.system ? 'exempt' : 'active'
}

/** Zone-file lines for the DS records of a signed zone. */
export function dsLines(zone: string, ds: string[]): string {
  return ds.map(d => `${zone}. IN DS ${d}`).join('\n')
}

/** Splits the AXFR allow list textarea into entries. */
export function parseAllowList(text: string): string[] {
  return text.split(/[\s,]+/).map(s => s.trim()).filter(Boolean)
}
