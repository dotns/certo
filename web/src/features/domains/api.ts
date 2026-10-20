import { request } from '@/shared/lib/http'

export interface Domain {
  domain: string
  subdomain: string
  cname_target: string
  owner?: string
}

export interface TxtRecord {
  domain: string
  value: string
  last_update: string
}

export const listDomains = () => request<Domain[]>('/api/domains')
export const addDomain = (domain: string) => request<Domain>('/api/domains', { method: 'POST', body: { domain } })
export const removeDomain = (domain: string) => request<void>(`/api/domains/${encodeURIComponent(domain)}`, { method: 'DELETE' })
export const listRecords = () => request<TxtRecord[]>('/api/records')
