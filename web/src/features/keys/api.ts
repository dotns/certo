import { request } from '@/shared/lib/http'

export interface ApiKey {
  id: number
  name: string
  key: string
  scope: string[]
  created_at: number
}

export const listKeys = () => request<ApiKey[]>('/api/keys')
export const createKey = (body: { name: string, scope: string[] }) => request<ApiKey>('/api/keys', { method: 'POST', body })
export const deleteKey = (id: number) => request<void>(`/api/keys/${id}`, { method: 'DELETE' })
