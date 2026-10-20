import { describe, expect, it } from 'vitest'
import { clientSnippets } from '../snippets'

describe('clientSnippets', () => {
  it('fills the httpreq snippets with the account', () => {
    const set = clientSnippets('httpreq', 'auth.example.org', 'alice')
    expect(set.note).toBeNull()
    expect(set.snippets).toHaveLength(4)
    expect(set.snippets[0]?.code).toContain('HTTPREQ_ENDPOINT=https://auth.example.org')
    expect(set.snippets[0]?.code).toContain('HTTPREQ_USERNAME=alice')
    expect(set.snippets[2]?.code).toContain('provider: httpreq')
  })

  it('fills the acme-dns snippets with the storage URL', () => {
    const set = clientSnippets('acmedns', 'auth.example.org', 'alice')
    expect(set.note).toContain('acme-dns')
    expect(set.snippets[0]?.code).toContain('ACME_DNS_STORAGE_BASE_URL=https://alice:<your-api-key>@auth.example.org/acmedns')
    expect(set.snippets[2]?.code).toContain('provider: acme-dns')
  })
})
