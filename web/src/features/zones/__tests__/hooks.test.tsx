import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useSession } from '@/shared/lib/session'
import { createWrapper, stubFetch } from '@/test/query'
import {
  useActivationMutation,
  useAddTsigKeyMutation,
  useCreateZoneMutation,
  useDeleteTsigKeyMutation,
  useDeleteZoneMutation,
  useDnssecQuery,
  usePatchRrsetsMutation,
  useRrsetsQuery,
  useSetDnssecMutation,
  useSetTransferMutation,
  useTransferQuery,
  useVerifyZoneMutation,
  useZoneQuery,
  useZonesQuery,
} from '../hooks'

const zone = {
  name: 'a.test',
  status: 'pending',
  exempt: false,
  system: false,
  verified_at: null,
  last_check: null,
  last_error: null,
  failed_checks: 0,
  created_at: 1,
  ns_target: 'ns.b.test',
}

describe('zone hooks', () => {
  beforeEach(() => useSession.setState({ token: 'jwt', adminKey: 'adm' }))
  afterEach(() => vi.unstubAllGlobals())

  it('refreshes zone lists after create, verify, activation and delete', async () => {
    let zones = [zone]
    const calls = stubFetch((call) => {
      if (call.method === 'POST' && call.url === '/api/zones') {
        zones = [...zones, { ...zone, name: 'b.test' }]
        return zones[1]
      }
      if (call.url.endsWith('/verify'))
        return { ...zone, status: 'active' }
      if (call.url.endsWith('/activation')) {
        zones = zones.map(z => ({ ...z, status: 'active', exempt: true }))
        return zones[0]
      }
      if (call.method === 'DELETE') {
        zones = zones.filter(z => `/api/zones/${z.name}` !== call.url)
        return null
      }
      return zones
    })
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({
      list: useZonesQuery('user'),
      create: useCreateZoneMutation('user'),
      verify: useVerifyZoneMutation('user'),
      activate: useActivationMutation(),
      remove: useDeleteZoneMutation('user'),
    }), { wrapper })
    await waitFor(() => expect(result.current.list.data).toHaveLength(1))
    await act(() => result.current.create.mutateAsync({ name: 'b.test' }))
    await waitFor(() => expect(result.current.list.data).toHaveLength(2))
    await act(() => result.current.verify.mutateAsync('a.test'))
    await act(() => result.current.activate.mutateAsync({ zone: 'a.test', exempt: true }))
    await waitFor(() => expect(result.current.list.data?.[0]?.exempt).toBe(true))
    await act(() => result.current.remove.mutateAsync('b.test'))
    await waitFor(() => expect(result.current.list.data).toHaveLength(1))
    expect(calls.map(c => `${c.method} ${c.url}`)).toContain('PUT /admin/zones/a.test/activation')
  })

  it('updates rrsets, dnssec and transfer caches from mutation results', async () => {
    const rrsets = [{ name: 'a.test.', type: 'NS', ttl: 3600, records: ['ns.b.test.'] }]
    const calls = stubFetch((call) => {
      if (call.url.endsWith('/rrsets'))
        return call.method === 'PATCH' ? [...rrsets, { name: 'a.test.', type: 'A', ttl: 300, records: ['192.0.2.1'] }] : rrsets
      if (call.url.endsWith('/dnssec'))
        return { enabled: call.method === 'PUT', nsec_mode: 'nsec' }
      if (call.url.endsWith('/axfr'))
        return { allow_from: call.method === 'PUT' ? ['192.0.2.0/24'] : [], tsig_keys: [] }
      if (call.url.endsWith('/tsig-keys'))
        return { name: 'k', algorithm: 'hmac-sha256', secret: 's' }
      if (call.method === 'DELETE')
        return null
      return { ...zone, status: 'active' }
    })
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({
      zone: useZoneQuery('user', 'a.test'),
      rrsets: useRrsetsQuery('user', 'a.test', true),
      dnssec: useDnssecQuery('user', 'a.test', true),
      transfer: useTransferQuery('user', 'a.test', true),
      patch: usePatchRrsetsMutation('user', 'a.test'),
      setDnssec: useSetDnssecMutation('user', 'a.test'),
      setTransfer: useSetTransferMutation('user', 'a.test'),
      addKey: useAddTsigKeyMutation('user', 'a.test'),
      deleteKey: useDeleteTsigKeyMutation('user', 'a.test'),
    }), { wrapper })
    await waitFor(() => expect(result.current.zone.data?.status).toBe('active'))
    await waitFor(() => expect(result.current.rrsets.data).toHaveLength(1))
    await waitFor(() => expect(result.current.dnssec.data?.enabled).toBe(false))
    await waitFor(() => expect(result.current.transfer.data?.allow_from).toEqual([]))
    await act(() => result.current.patch.mutateAsync([{ name: '@', type: 'A', records: ['192.0.2.1'] }]))
    await waitFor(() => expect(result.current.rrsets.data).toHaveLength(2))
    await act(() => result.current.setDnssec.mutateAsync({ enabled: true, nsec_mode: 'nsec' }))
    await waitFor(() => expect(result.current.dnssec.data?.enabled).toBe(true))
    await act(() => result.current.setTransfer.mutateAsync(['192.0.2.0/24']))
    await waitFor(() => expect(result.current.transfer.data?.allow_from).toEqual(['192.0.2.0/24']))
    const key = await act(() => result.current.addKey.mutateAsync({ name: 'k', algorithm: 'hmac-sha256' }))
    expect(key.secret).toBe('s')
    await act(() => result.current.deleteKey.mutateAsync('k'))
    const transferFetches = calls.filter(c => c.method === 'GET' && c.url.endsWith('/axfr'))
    expect(transferFetches.length).toBeGreaterThanOrEqual(2)
  })
})
