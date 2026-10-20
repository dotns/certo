import { request } from '@/shared/lib/http'

export type ZoneStatus = 'pending' | 'active'

export interface RrSet {
  name: string
  type: string
  ttl: number
  records: string[]
}

export interface Zone {
  name: string
  status: ZoneStatus
  exempt: boolean
  system: boolean
  verified_at: number | null
  last_check: number | null
  last_error: string | null
  failed_checks: number
  created_at: number
  ns_target: string
  owner?: string
  rrsets?: RrSet[]
}

export interface RrSetChange {
  name: string
  type: string
  ttl?: number
  changetype?: 'REPLACE' | 'DELETE'
  records?: string[]
}

export interface Dnssec {
  enabled: boolean
  nsec_mode: string
  dnskey?: string
  ds?: string[]
}

export interface TsigKey {
  name: string
  algorithm: string
  secret?: string
}

export interface Transfer {
  allow_from: string[]
  tsig_keys: TsigKey[]
}

/** `user` talks to `/api/zones` with the session; `admin` to `/admin/zones` with the admin key. */
export type Scope = 'user' | 'admin'

function base(scope: Scope, zone?: string): string {
  const prefix = scope === 'admin' ? '/admin/zones' : '/api/zones'
  return zone === undefined ? prefix : `${prefix}/${encodeURIComponent(zone)}`
}

const opts = (scope: Scope, extra: object = {}) => ({ admin: scope === 'admin', ...extra })

export const listZones = (scope: Scope) => request<Zone[]>(base(scope), opts(scope))
export function createZone(scope: Scope, body: { name: string, user_id?: number }) {
  return request<Zone>(base(scope), opts(scope, { method: 'POST', body }))
}
export const getZone = (scope: Scope, zone: string) => request<Zone>(base(scope, zone), opts(scope))
export const deleteZone = (scope: Scope, zone: string) => request<void>(base(scope, zone), opts(scope, { method: 'DELETE' }))
export const verifyZone = (scope: Scope, zone: string) => request<Zone>(`${base(scope, zone)}/verify`, opts(scope, { method: 'POST' }))
export function setActivation(zone: string, exempt: boolean) {
  return request<Zone>(`${base('admin', zone)}/activation`, { admin: true, method: 'PUT', body: { exempt } })
}
export const listRrsets = (scope: Scope, zone: string) => request<RrSet[]>(`${base(scope, zone)}/rrsets`, opts(scope))
export function patchRrsets(scope: Scope, zone: string, rrsets: RrSetChange[]) {
  return request<RrSet[]>(`${base(scope, zone)}/rrsets`, opts(scope, { method: 'PATCH', body: { rrsets } }))
}
export const getDnssec = (scope: Scope, zone: string) => request<Dnssec>(`${base(scope, zone)}/dnssec`, opts(scope))
export function setDnssec(scope: Scope, zone: string, body: { enabled: boolean, nsec_mode: string }) {
  return request<Dnssec>(`${base(scope, zone)}/dnssec`, opts(scope, { method: 'PUT', body }))
}
export const getTransfer = (scope: Scope, zone: string) => request<Transfer>(`${base(scope, zone)}/axfr`, opts(scope))
export function setTransfer(scope: Scope, zone: string, allow_from: string[]) {
  return request<Transfer>(`${base(scope, zone)}/axfr`, opts(scope, { method: 'PUT', body: { allow_from } }))
}
export function addTsigKey(scope: Scope, zone: string, body: { name: string, algorithm: string }) {
  return request<TsigKey>(`${base(scope, zone)}/tsig-keys`, opts(scope, { method: 'POST', body }))
}
export function deleteTsigKey(scope: Scope, zone: string, name: string) {
  return request<void>(`${base(scope, zone)}/tsig-keys/${encodeURIComponent(name)}`, opts(scope, { method: 'DELETE' }))
}
