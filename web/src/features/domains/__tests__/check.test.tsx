import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWrapper, stubFetch } from '@/test/query'
import { useCheckDomainMutation } from '../hooks'

describe('useCheckDomainMutation', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('calls the check endpoint for the domain', async () => {
    const calls = stubFetch(() => ({ name: '_acme-challenge.a.test', expected: 'x.s.test', observed: null, ok: false }))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useCheckDomainMutation(), { wrapper })
    await act(() => result.current.mutateAsync('a.test'))
    await waitFor(() => expect(result.current.data?.ok).toBe(false))
    expect(calls[0]?.url).toBe('/api/domains/a.test/check')
  })
})
