import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useSession } from '@/shared/lib/session'
import { addTsigKey, createZone, deleteTsigKey, deleteZone, getDnssec, getTransfer, getZone, listRrsets, listZones, patchRrsets, setActivation, setDnssec, setTransfer, verifyZone } from '../api'

describe('zones api', () => {
  const calls: Array<[string, RequestInit]> = []
  beforeEach(() => {
    calls.length = 0
    useSession.setState({ token: 'jwt', adminKey: 'adm' })
    vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
      calls.push([url, init])
      return new Response('{}', { status: 200 })
    }))
  })
  afterEach(() => vi.unstubAllGlobals())

  it('addresses the user and admin prefixes with matching credentials', async () => {
    await listZones('user')
    await listZones('admin')
    expect(calls[0]?.[0]).toBe('/api/zones')
    expect((calls[0]?.[1].headers as Record<string, string>).Authorization).toBe('Bearer jwt')
    expect(calls[1]?.[0]).toBe('/admin/zones')
    expect((calls[1]?.[1].headers as Record<string, string>)['X-Admin-Key']).toBe('adm')
  })

  it('builds every zone route', async () => {
    await createZone('admin', { name: 'a.test', user_id: 3 })
    await getZone('user', 'a.test')
    await deleteZone('user', 'a.test')
    await verifyZone('user', 'a.test')
    await setActivation('a.test', true)
    await listRrsets('user', 'a.test')
    await patchRrsets('user', 'a.test', [{ name: '@', type: 'A', records: ['192.0.2.1'] }])
    await getDnssec('user', 'a.test')
    await setDnssec('user', 'a.test', { enabled: true, nsec_mode: 'nsec3' })
    await getTransfer('user', 'a.test')
    await setTransfer('user', 'a.test', ['127.0.0.1'])
    await addTsigKey('user', 'a.test', { name: 'k', algorithm: 'hmac-sha256' })
    await deleteTsigKey('user', 'a.test', 'k/1')
    expect(calls.map(([url, init]) => `${init.method} ${url}`)).toEqual([
      'POST /admin/zones',
      'GET /api/zones/a.test',
      'DELETE /api/zones/a.test',
      'POST /api/zones/a.test/verify',
      'PUT /admin/zones/a.test/activation',
      'GET /api/zones/a.test/rrsets',
      'PATCH /api/zones/a.test/rrsets',
      'GET /api/zones/a.test/dnssec',
      'PUT /api/zones/a.test/dnssec',
      'GET /api/zones/a.test/axfr',
      'PUT /api/zones/a.test/axfr',
      'POST /api/zones/a.test/tsig-keys',
      'DELETE /api/zones/a.test/tsig-keys/k%2F1',
    ])
    expect(calls[0]?.[1].body).toBe('{"name":"a.test","user_id":3}')
    expect(calls[4]?.[1].body).toBe('{"exempt":true}')
    expect(calls[6]?.[1].body).toBe('{"rrsets":[{"name":"@","type":"A","records":["192.0.2.1"]}]}')
  })
})
