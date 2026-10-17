import { getToken } from './auth';

const BASE = '';

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...((options.headers as Record<string, string>) || {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  const res = await fetch(`${BASE}${path}`, { ...options, headers });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.message || body.error || `HTTP ${res.status}`);
  }
  if (res.status === 204) return {} as T;
  return res.json();
}

export interface AuthResponse {
  token: string;
  username: string;
}

export interface ProfileResponse {
  username: string;
}

export interface Domain {
  domain: string;
  cname_target: string;
}

export interface TXTRecord {
  domain: string;
  value: string;
  last_update: string;
}

export interface APIKeyItem {
  id: number;
  name: string;
  key: string;
  scope: string[];
  created_at: number;
}

export interface InfoResponse {
  base_domain: string;
  api_domain: string;
  version: string;
  allow_zone_creation?: boolean;
}

export interface Zone {
  name: string;
  serial: number;
  dnssec: boolean;
  created_at: number;
  records?: ZoneRecord[];
}

export interface ZoneRecord {
  id: number;
  name: string;
  type: string;
  ttl: number;
  data: string;
}

export interface RecordInput {
  name: string;
  type: string;
  ttl?: number;
  data: string;
}

export interface Dnssec {
  enabled: boolean;
  nsec_mode: string;
  dnskey?: string;
  ds?: string;
}

export interface TsigKey {
  name: string;
  algorithm: string;
  created_at: number;
  secret?: string;
}

export interface Transfer {
  allow_from: string[];
  tsig_keys: TsigKey[];
}

const zonePath = (zone: string) => `/api/zones/${encodeURIComponent(zone)}`;

export const api = {
  register: (username: string, password: string) =>
    request<AuthResponse>('/api/register', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  login: (username: string, password: string) =>
    request<AuthResponse>('/api/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  getDomains: () => request<Domain[]>('/api/domains'),

  addDomain: (domain: string) =>
    request<Domain>('/api/domains', {
      method: 'POST',
      body: JSON.stringify({ domain }),
    }),

  removeDomain: (domain: string) =>
    request<void>(`/api/domains/${encodeURIComponent(domain)}`, {
      method: 'DELETE',
    }),

  getRecords: () => request<TXTRecord[]>('/api/records'),

  getKeys: () => request<APIKeyItem[]>('/api/keys'),

  createKey: (name: string, scope: string[]) =>
    request<APIKeyItem>('/api/keys', {
      method: 'POST',
      body: JSON.stringify({ name, scope }),
    }),

  deleteKey: (id: number) =>
    request<void>(`/api/keys/${id}`, { method: 'DELETE' }),

  getProfile: () => request<ProfileResponse>('/api/profile'),

  getInfo: () => request<InfoResponse>('/api/info'),

  getZones: () => request<Zone[]>('/api/zones'),

  createZone: (name: string) =>
    request<Zone>('/api/zones', { method: 'POST', body: JSON.stringify({ name }) }),

  getZone: (zone: string) => request<Zone>(zonePath(zone)),

  deleteZone: (zone: string) => request<void>(zonePath(zone), { method: 'DELETE' }),

  addRecord: (zone: string, rec: RecordInput) =>
    request<ZoneRecord>(`${zonePath(zone)}/records`, { method: 'POST', body: JSON.stringify(rec) }),

  updateRecord: (zone: string, id: number, rec: RecordInput) =>
    request<ZoneRecord>(`${zonePath(zone)}/records/${id}`, { method: 'PUT', body: JSON.stringify(rec) }),

  deleteRecord: (zone: string, id: number) =>
    request<void>(`${zonePath(zone)}/records/${id}`, { method: 'DELETE' }),

  getDnssec: (zone: string) => request<Dnssec>(`${zonePath(zone)}/dnssec`),

  setDnssec: (zone: string, enabled: boolean, nsec_mode: string) =>
    request<Dnssec>(`${zonePath(zone)}/dnssec`, { method: 'PUT', body: JSON.stringify({ enabled, nsec_mode }) }),

  getTransfer: (zone: string) => request<Transfer>(`${zonePath(zone)}/axfr`),

  setTransfer: (zone: string, allow_from: string[]) =>
    request<Transfer>(`${zonePath(zone)}/axfr`, { method: 'PUT', body: JSON.stringify({ allow_from }) }),

  addTsigKey: (zone: string, name: string, algorithm: string) =>
    request<TsigKey>(`${zonePath(zone)}/tsig-keys`, { method: 'POST', body: JSON.stringify({ name, algorithm }) }),

  deleteTsigKey: (zone: string, name: string) =>
    request<void>(`${zonePath(zone)}/tsig-keys/${encodeURIComponent(name)}`, { method: 'DELETE' }),
};
