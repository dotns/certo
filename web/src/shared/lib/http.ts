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

/** Human-readable text for an error thrown by {@link request}. */
export function errorMessage(err: unknown, fallback = 'Something went wrong'): string {
  if (err instanceof ApiError)
    return err.message === err.code ? describe(err.code) : `${describe(err.code)}: ${err.message}`
  if (err instanceof Error && err.message)
    return err.message
  return fallback
}

const DESCRIPTIONS: Record<string, string> = {
  invalid_credentials: 'Wrong username or password',
  user_exists: 'That username is taken',
  invalid_username: 'Usernames use letters, digits, dots, dashes and underscores',
  invalid_password: 'The password is too short (at least 6 characters)',
  session_required: 'Sign in with your password to do this',
  user_not_found: 'User not found',
  domain_not_found: 'Domain not found',
  username_taken: 'That username is taken',
  name_required: 'A name is required',
  invalid_json: 'The request was malformed',
  invalid_admin_key: 'Wrong admin key',
  admin_key_not_configured: 'The admin API is disabled on this server',
  registration_disabled: 'Registration is disabled on this server',
  domain_not_in_scope: 'This API key does not cover that domain',
  domain_exists: 'That domain is already added',
  zone_not_allowed: 'That zone cannot be created here',
  zone_already_exists: 'That zone already exists',
  zone_not_found: 'Zone not found',
  zone_not_active: 'The zone is not active yet',
  check_rate_limited: 'Please wait a minute before checking again',
  check_failed: 'The delegation check could not be completed',
  tsig_key_exists: 'A TSIG key with that name exists',
  tsig_key_not_found: 'TSIG key not found',
  invalid_cidr: 'Enter IP addresses or CIDR ranges',
  invalid_nsec_mode: 'Choose NSEC or NSEC3',
  pdns_rejected: 'The nameserver rejected the record',
}

function describe(code: string): string {
  return DESCRIPTIONS[code] ?? code.replace(/_/g, ' ')
}
