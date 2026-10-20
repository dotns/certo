import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useSession } from '@/shared/lib/session'
import { createWrapper, stubFetch } from '@/test/query'
import {
  useAddDomainForMutation,
  useAdminDomainsQuery,
  useAdminRecordsQuery,
  useAdminUsersQuery,
  useCreateUserMutation,
  useDeleteUserMutation,
  useRemoveDomainEverywhereMutation,
} from '../hooks'

describe('admin hooks', () => {
  beforeEach(() => useSession.setState({ token: 'jwt', adminKey: null }))
  afterEach(() => vi.unstubAllGlobals())

  it('stays idle without an admin key', () => {
    stubFetch(() => [])
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useAdminUsersQuery(), { wrapper })
    expect(result.current.fetchStatus).toBe('idle')
  })

  it('manages users and domains with the admin key', async () => {
    useSession.getState().setAdminKey('adm')
    let users = [{ id: 1, username: 'alice', created_at: 1 }]
    let domains = [{ domain: 'a.test', subdomain: 'x', cname_target: 'x.b.test', owner: 'alice' }]
    const calls = stubFetch((call) => {
      if (call.url === '/admin/records')
        return []
      if (call.url === '/admin/users' && call.method === 'POST') {
        users = [...users, { id: 2, username: 'bob', created_at: 2 }]
        return users[1]
      }
      if (call.url.startsWith('/admin/users/') && call.method === 'DELETE') {
        users = users.filter(u => `/admin/users/${u.id}` !== call.url)
        return null
      }
      if (call.url === '/admin/domains' && call.method === 'POST') {
        const created = { domain: 'c.test', subdomain: 'y', cname_target: 'y.b.test', owner: 'bob' }
        domains = [...domains, created]
        return created
      }
      if (call.url.startsWith('/admin/domains/') && call.method === 'DELETE') {
        domains = domains.filter(d => `/admin/domains/${d.domain}` !== call.url)
        return null
      }
      return call.url === '/admin/users' ? users : domains
    })
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({
      users: useAdminUsersQuery(),
      domains: useAdminDomainsQuery(),
      records: useAdminRecordsQuery(),
      createUser: useCreateUserMutation(),
      deleteUser: useDeleteUserMutation(),
      addDomain: useAddDomainForMutation(),
      removeDomain: useRemoveDomainEverywhereMutation(),
    }), { wrapper })
    await waitFor(() => expect(result.current.users.data).toHaveLength(1))
    await waitFor(() => expect(result.current.records.data).toEqual([]))
    await act(() => result.current.createUser.mutateAsync({ username: 'bob', password: 'secret' }))
    await waitFor(() => expect(result.current.users.data).toHaveLength(2))
    await act(() => result.current.addDomain.mutateAsync({ user_id: 2, domain: 'c.test' }))
    await waitFor(() => expect(result.current.domains.data).toHaveLength(2))
    await act(() => result.current.removeDomain.mutateAsync('a.test'))
    await waitFor(() => expect(result.current.domains.data).toHaveLength(1))
    await act(() => result.current.deleteUser.mutateAsync(1))
    await waitFor(() => expect(result.current.users.data).toEqual([expect.objectContaining({ id: 2 })]))
    expect(calls.every(c => c.url.startsWith('/admin/'))).toBe(true)
  })
})
