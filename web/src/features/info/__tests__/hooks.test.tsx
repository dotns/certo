import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { createWrapper, stubFetch } from '@/test/query'
import { useInfoQuery } from '../hooks'

describe('useInfoQuery', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('loads the public server info', async () => {
    const calls = stubFetch(() => ({ provider: 'certo', version: '0.1.0', base_domain: 'b.test', api_domain: 'api.b.test', ns_target: 'ns.b.test', capabilities: [] }))
    const { wrapper } = createWrapper()
    const { result } = renderHook(() => useInfoQuery(), { wrapper })
    await waitFor(() => expect(result.current.data?.ns_target).toBe('ns.b.test'))
    expect(calls[0]?.url).toBe('/api/info')
  })
})
