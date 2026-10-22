import i18next from 'i18next'
import { useSession } from './session'

/** An error answer from the API: `{"error": code, "message"?: details}`. */
export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message?: string) {
    super(message ?? code)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
  body?: unknown
  /** Send the admin key instead of the session token. */
  admin?: boolean
  signal?: AbortSignal
}

/** Codes the API answers with when the session token is no longer usable. */
const SESSION_ENDED = new Set(['invalid_token', 'token_expired', 'user_not_found'])

/** Calls the certo API on the same origin; JSON in, JSON out. */
export async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const { token, adminKey } = useSession.getState()
  const headers: Record<string, string> = {}
  if (opts.admin) {
    headers['X-Admin-Key'] = adminKey ?? ''
  }
  else if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  if (opts.body !== undefined)
    headers['Content-Type'] = 'application/json'
  const res = await fetch(path, {
    method: opts.method ?? 'GET',
    headers,
    body: opts.body === undefined ? undefined : JSON.stringify(opts.body),
    signal: opts.signal,
  })
  if (res.status === 204)
    return undefined as T
  const text = await res.text()
  let data: unknown
  try {
    data = text ? JSON.parse(text) : undefined
  }
  catch {
    data = undefined
  }
  if (!res.ok) {
    const body = (typeof data === 'object' && data !== null ? data : {}) as { error?: string, message?: string }
    const code = body.error ?? `http_${res.status}`
    if (res.status === 401 && !opts.admin && SESSION_ENDED.has(code))
      useSession.getState().signOut()
    throw new ApiError(res.status, code, body.message)
  }
  return data as T
}

/** Human-readable text for an error thrown by {@link request}, in the active language. */
export function errorMessage(err: unknown, fallback = i18next.t('errors.generic')): string {
  if (err instanceof ApiError)
    return err.message === err.code ? describe(err.code) : `${describe(err.code)}: ${err.message}`
  if (err instanceof Error && err.message)
    return err.message
  return fallback
}

function describe(code: string): string {
  return i18next.t(`errors.${code}`, { defaultValue: code.replace(/_/g, ' ') })
}
