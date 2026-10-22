import { request } from '@/shared/lib/http'

export interface ServerInfo {
  provider: string
  version: string
  base_domain: string
  base_zone: string
  api_domain: string
  ns_target: string
  capabilities: string[]
  registration: boolean
}

export const getInfo = () => request<ServerInfo>('/api/info')
