import { request } from '@/shared/lib/http'

export interface AuthResponse {
  token: string
  username: string
}

export interface Profile {
  username: string
}

export interface Credentials {
  username: string
  password: string
}

export const login = (body: Credentials) => request<AuthResponse>('/api/login', { method: 'POST', body })
export const register = (body: Credentials) => request<AuthResponse>('/api/register', { method: 'POST', body })
export const getProfile = () => request<Profile>('/api/profile')
export const deleteAccount = () => request<void>('/api/profile', { method: 'DELETE' })

export interface PasswordChange {
  current_password: string
  new_password: string
}

export const changePassword = (body: PasswordChange) => request<void>('/api/profile/password', { method: 'PUT', body })
