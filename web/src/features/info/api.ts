import { request } from '@/shared/lib/http'

export interface ServerInfo {
  provider: string
  version: string
  base_domain: string
  api_domain: string
  ns_target: string
  capabilities: string[]
}

export const getInfo = () => request<ServerInfo>('/api/info')
