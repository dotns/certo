import { describe, expect, it } from 'vitest'
import { CLIENTS, clientSnippets, KEY_PLACEHOLDER } from '../snippets'

describe('clientSnippets', () => {
  it('fills the httpreq snippets with the account and key', () => {
    const clients = clientSnippets('httpreq', 'auth.example.org', 'alice', 'k-1')
    expect(clients.map(c => c.id)).toEqual(CLIENTS.map(c => c.id))
    const lego = clients.find(c => c.id === 'lego')!
    expect(lego.snippets[0]?.code).toContain('HTTPREQ_ENDPOINT=https://auth.example.org')
    expect(lego.snippets[0]?.code).toContain('HTTPREQ_USERNAME=alice')
    expect(lego.snippets[0]?.code).toContain('HTTPREQ_PASSWORD=k-1')
    expect(clients.find(c => c.id === 'traefik')!.snippets[0]?.code).toContain('provider: httpreq')
    expect(clients.find(c => c.id === 'caddy')!.snippets).toHaveLength(0)
  })

  it('fills the acme-dns snippets with the storage URL and the placeholder key', () => {
    const clients = clientSnippets('acmedns', 'auth.example.org', 'alice')
    const lego = clients.find(c => c.id === 'lego')!
    expect(lego.snippets[0]?.code).toContain(`ACME_DNS_STORAGE_BASE_URL=https://alice:${KEY_PLACEHOLDER}@auth.example.org/acmedns`)
    expect(clients.find(c => c.id === 'traefik')!.snippets[0]?.code).toContain('provider: acme-dns')
    expect(clients.find(c => c.id === 'caddy')!.snippets[0]?.code).toContain('dns acmedns')
    expect(clients.find(c => c.id === 'acmesh')!.snippets[0]?.code).toContain('ACMEDNS_USERNAME="alice"')
    const secret = clients.find(c => c.id === 'certmanager')!.snippets[0]?.code ?? ''
    expect(secret).toContain('"username": "alice"')
    expect(clients.find(c => c.id === 'certmanager')!.snippets[1]?.code).toContain('host: https://auth.example.org')
  })
})
