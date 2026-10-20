import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWrapper, stubFetch } from '@/test/query'
import { useCreateKeyMutation, useDeleteKeyMutation, useKeysQuery } from '../hooks'

describe('key hooks', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('lists, creates and deletes keys with list refreshes', async () => {
    let keys = [{ id: 1, name: 'a', key: 'k1', scope: ['*'], created_at: 1 }]
    const calls = stubFetch((call) => {
      if (call.method === 'POST') {
        keys = [...keys, { id: 2, name: 'b', key: 'k2', scope: ['*'], created_at: 2 }]
        return keys[1]
      }
      if (call.method === 'DELETE') {
        keys = keys.filter(k => `/api/keys/${k.id}` !== call.url)
        return null
      }
      return keys
    })
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => ({ list: useKeysQuery(), create: useCreateKeyMutation(), remove: useDeleteKeyMutation() }), { wrapper })
    await waitFor(() => expect(result.current.list.data).toHaveLength(1))
    await act(() => result.current.create.mutateAsync({ name: 'b', scope: ['*'] }))
    await waitFor(() => expect(result.current.list.data).toHaveLength(2))
    await act(() => result.current.remove.mutateAsync(1))
    await waitFor(() => expect(result.current.list.data).toEqual([expect.objectContaining({ id: 2 })]))
    expect(calls.some(c => c.method === 'DELETE' && c.url === '/api/keys/1')).toBe(true)
  })
})
