import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWrapper, stubFetch } from '@/test/query'
import { useChangePasswordMutation } from '../hooks'

describe('useChangePasswordMutation', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('puts the new password', async () => {
    const calls = stubFetch(() => null)
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useChangePasswordMutation(), { wrapper })
    await act(() => result.current.mutateAsync({ current_password: 'a', new_password: 'b' }))
    expect(calls[0]).toMatchObject({ method: 'PUT', url: '/api/profile/password', body: { current_password: 'a', new_password: 'b' } })
  })
})
