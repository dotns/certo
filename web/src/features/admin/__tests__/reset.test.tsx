import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { useSession } from '@/shared/lib/session'
import { createWrapper, stubFetch } from '@/test/query'
import { useResetPasswordMutation } from '../hooks'

describe('useResetPasswordMutation', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('puts the password with the admin key', async () => {
    useSession.getState().setAdminKey('adm')
    const calls = stubFetch(() => null)
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useResetPasswordMutation(), { wrapper })
    await act(() => result.current.mutateAsync({ id: 7, password: 'secret7' }))
    expect(calls[0]).toMatchObject({ method: 'PUT', url: '/admin/users/7/password', body: { password: 'secret7' } })
  })
})
