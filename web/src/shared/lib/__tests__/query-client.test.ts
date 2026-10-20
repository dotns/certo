import { describe, expect, it } from 'vitest'
import { queryClient } from '../query-client'

describe('queryClient', () => {
  it('keeps data fresh for 30 seconds and retries once', () => {
    expect(queryClient.getDefaultOptions().queries).toMatchObject({ staleTime: 30_000, retry: 1 })
  })
})
