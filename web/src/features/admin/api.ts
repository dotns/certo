import type { Domain, TxtRecord } from '@/features/domains/api'
import { request } from '@/shared/lib/http'

export interface AdminUser {
  id: number
  username: string
  created_at: number
}

export const listUsers = () => request<AdminUser[]>('/admin/users', { admin: true })
export function createUser(body: { username: string, password: string }) {
  return request<AdminUser>('/admin/users', { admin: true, method: 'POST', body })
}
export const deleteUser = (id: number) => request<void>(`/admin/users/${id}`, { admin: true, method: 'DELETE' })
export const listAllDomains = () => request<Domain[]>('/admin/domains', { admin: true })
export function addDomainFor(body: { user_id: number, domain: string }) {
  return request<Domain>('/admin/domains', { admin: true, method: 'POST', body })
}
export function removeDomainEverywhere(domain: string) {
  return request<void>(`/admin/domains/${encodeURIComponent(domain)}`, { admin: true, method: 'DELETE' })
}
export const listAllRecords = () => request<TxtRecord[]>('/admin/records', { admin: true })
export function resetPassword(body: { id: number, password: string }) {
  return request<void>(`/admin/users/${body.id}/password`, { admin: true, method: 'PUT', body: { password: body.password } })
}

export interface ZoneVersions {
  name: string
  db: number | null
  pdns: number | null
}

export interface SyncStatus {
  db_version: number
  pdns_version: number
  zones: ZoneVersions[]
}

export interface SyncReport extends SyncStatus {
  created: number
  removed: number
  rrsets_changed: number
  keys_imported: number
  records_adopted: number
}

export const getSyncStatus = () => request<SyncStatus>('/admin/sync', { admin: true })
export const runSync = () => request<SyncReport>('/admin/sync', { admin: true, method: 'POST' })

export interface CleanupPreview {
  accounts: number
  older_than_days: number
}

export const deleteUsers = (ids: number[]) => request<{ deleted: number }>('/admin/users/delete', { admin: true, method: 'POST', body: { ids } })
export const deleteDomains = (domains: string[]) => request<{ deleted: number }>('/admin/domains/delete', { admin: true, method: 'POST', body: { domains } })
export const getCleanupPreview = (days: number) => request<CleanupPreview>(`/admin/cleanup?older_than_days=${days}`, { admin: true })
export const runCleanup = (days: number) => request<{ deleted: number }>('/admin/cleanup', { admin: true, method: 'POST', body: { older_than_days: days } })
