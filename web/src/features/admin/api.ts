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
