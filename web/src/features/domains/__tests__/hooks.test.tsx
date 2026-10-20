import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWrapper, stubFetch } from '@/test/query'
import { useAddDomainMutation, useDomainsQuery, useRecordsQuery, useRemoveDomainMutation } from '../hooks'

describe('domain hooks', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('lists domains and refreshes after adding and removing', async () => {
    let domains = [{ domain: 'a.test', subdomain: 'abc', cname_target: 'abc.base.test' }]
    const calls = stubFetch((call) => {
      if (call.url === '/api/records')
        return [{ domain: '_acme-challenge.a.test', value: 'tok', last_update: '2026-10-18T00:00:00Z' }]
      if (call.method === 'POST') {
        const created = { domain: (call.body as { domain: string }).domain, subdomain: 'def', cname_target: 'def.base.test' }
        domains = [...domains, created]
        return created
      }
      if (call.method === 'DELETE') {
        domains = domains.filter(d => `/api/domains/${encodeURIComponent(d.domain)}` !== call.url)
        return null
      }
      return domains
    })
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({
      list: useDomainsQuery(),
      records: useRecordsQuery(),
      add: useAddDomainMutation(),
      remove: useRemoveDomainMutation(),
    }), { wrapper })
    await waitFor(() => expect(result.current.list.data).toHaveLength(1))
    await waitFor(() => expect(result.current.records.data).toHaveLength(1))
    await act(() => result.current.add.mutateAsync('b.test'))
    await waitFor(() => expect(result.current.list.data).toHaveLength(2))
    await act(() => result.current.remove.mutateAsync('a.test'))
    await waitFor(() => expect(result.current.list.data).toEqual([expect.objectContaining({ domain: 'b.test' })]))
    expect(calls.some(c => c.method === 'DELETE' && c.url === '/api/domains/a.test')).toBe(true)
  })
})
