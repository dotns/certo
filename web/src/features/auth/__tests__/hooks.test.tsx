import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useSession } from '@/shared/lib/session'
import { createWrapper, stubFetch } from '@/test/query'
import { useDeleteAccountMutation, useLoginMutation, useProfileQuery, useRegisterMutation, useSignOut } from '../hooks'

describe('auth hooks', () => {
  beforeEach(() => useSession.setState({ token: null, adminKey: null }))
  afterEach(() => vi.unstubAllGlobals())

  it('signs in after login and register', async () => {
    const calls = stubFetch(() => ({ token: 'jwt', username: 'alice' }))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({ login: useLoginMutation(), register: useRegisterMutation() }), { wrapper })
    await act(() => result.current.login.mutateAsync({ username: 'alice', password: 'pw' }))
    expect(useSession.getState().token).toBe('jwt')
    useSession.getState().signOut()
    await act(() => result.current.register.mutateAsync({ username: 'bob', password: 'pw' }))
    expect(useSession.getState().token).toBe('jwt')
    expect(calls.map(c => `${c.method} ${c.url}`)).toEqual(['POST /api/login', 'POST /api/register'])
  })

  it('loads the profile only with a token and clears everything on sign out', async () => {
    stubFetch(() => ({ username: 'alice' }))
    const { wrapper, client } = createWrapper()
    const { result, rerender } = renderHook(() => ({ profile: useProfileQuery(), signOut: useSignOut() }), { wrapper })
    expect(result.current.profile.fetchStatus).toBe('idle')
    act(() => useSession.getState().signIn('jwt'))
    rerender()
    await waitFor(() => expect(result.current.profile.data).toEqual({ username: 'alice' }))
    act(() => result.current.signOut())
    expect(useSession.getState().token).toBeNull()
    expect(client.getQueryData(['profile'])).toBeUndefined()
  })

  it('deletes the account and signs out', async () => {
    const calls = stubFetch(() => null)
    useSession.getState().signIn('jwt')
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useDeleteAccountMutation(), { wrapper })
    await act(() => result.current.mutateAsync())
    expect(calls[0]).toMatchObject({ method: 'DELETE', url: '/api/profile' })
    expect(useSession.getState().token).toBeNull()
  })
})
