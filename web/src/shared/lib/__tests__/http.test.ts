import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, errorMessage, request } from '../http'
import { useSession } from '../session'

function mockFetch(status: number, body: unknown | null, headers: Record<string, string> = {}) {
  const text = body === null ? null : JSON.stringify(body)
  const fn = vi.fn(async () => new Response(text, { status, headers }))
  vi.stubGlobal('fetch', fn)
  return fn
}

describe('request', () => {
  beforeEach(() => {
    useSession.setState({ token: null, adminKey: null })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the session token and JSON body', async () => {
    useSession.getState().signIn('jwt-1')
    const fetch = mockFetch(200, { ok: true })
    const res = await request<{ ok: boolean }>('/api/domains', { method: 'POST', body: { domain: 'a.test' } })
    expect(res).toEqual({ ok: true })
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/domains')
    expect(init.method).toBe('POST')
    expect(init.body).toBe('{"domain":"a.test"}')
    expect((init.headers as Record<string, string>).Authorization).toBe('Bearer jwt-1')
    expect((init.headers as Record<string, string>)['Content-Type']).toBe('application/json')
  })

  it('sends the admin key instead of the token when asked', async () => {
    useSession.getState().signIn('jwt-1')
    useSession.getState().setAdminKey('secret')
    const fetch = mockFetch(200, [])
    await request('/admin/users', { admin: true })
    const headers = (fetch.mock.calls[0] as unknown as [string, RequestInit])[1].headers as Record<string, string>
    expect(headers['X-Admin-Key']).toBe('secret')
    expect(headers.Authorization).toBeUndefined()
  })

  it('returns undefined for 204', async () => {
    mockFetch(204, null)
    await expect(request('/api/keys/1', { method: 'DELETE' })).resolves.toBeUndefined()
  })

  it('throws ApiError with code and message', async () => {
    mockFetch(400, { error: 'invalid_name', message: 'bad label' })
    const err = await request<never>('/api/zones').catch((e: unknown) => e as ApiError)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.status).toBe(400)
    expect(err.code).toBe('invalid_name')
    expect(err.message).toBe('bad label')
  })

  it('falls back to an http code when the body is not JSON', async () => {
    mockFetch(502, null)
    const err = await request<never>('/api/zones').catch((e: unknown) => e as ApiError)
    expect(err.code).toBe('http_502')
  })

  it('signs out when the token is rejected', async () => {
    useSession.getState().signIn('jwt-1')
    mockFetch(401, { error: 'invalid_token' })
    await request('/api/profile').catch(() => {})
    expect(useSession.getState().token).toBeNull()
  })

  it('keeps the session when the admin key is rejected', async () => {
    useSession.getState().signIn('jwt-1')
    useSession.getState().setAdminKey('nope')
    mockFetch(401, { error: 'invalid_admin_key' })
    await request('/admin/users', { admin: true }).catch(() => {})
    expect(useSession.getState().token).toBe('jwt-1')
  })
})

describe('errorMessage', () => {
  it('describes known codes and appends details', () => {
    expect(errorMessage(new ApiError(409, 'zone_already_exists'))).toBe('That zone already exists')
    expect(errorMessage(new ApiError(400, 'invalid_name', 'bad label'))).toBe('invalid name: bad label')
    expect(errorMessage(new Error('boom'))).toBe('boom')
    expect(errorMessage('x', 'fallback')).toBe('fallback')
  })
})
