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
    throw new Error(body.error || `HTTP ${res.status}`);
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
}

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
};
