export type Protocol = 'httpreq' | 'acmedns'

export interface Snippet {
  title: string
  code: string
}

export interface SnippetSet {
  note: string | null
  snippets: Snippet[]
}

const TRAEFIK_BASE = `certificatesResolvers:
  letsencrypt:
    acme:
      email: admin@example.com
      storage: /data/ssl/acme.json
      dnsChallenge:`

/** Client configuration examples for the given protocol and account. */
export function clientSnippets(protocol: Protocol, apiDomain: string, username: string): SnippetSet {
  const key = '<your-api-key>'
  if (protocol === 'httpreq') {
    return {
      note: null,
      snippets: [
        { title: 'Environment variables', code: `HTTPREQ_ENDPOINT=https://${apiDomain}\nHTTPREQ_USERNAME=${username}\nHTTPREQ_PASSWORD=${key}\nLEGO_DISABLE_CNAME_SUPPORT=true` },
        { title: 'lego', code: `lego --dns httpreq --dns.propagation-disable-ans \\\n  --domains example.com --domains "*.example.com" \\\n  --email admin@example.com --accept-tos run` },
        { title: 'Traefik: traefik.yml', code: `${TRAEFIK_BASE}\n        provider: httpreq\n        propagation:\n          disableChecks: true` },
        { title: 'Traefik: docker-compose environment', code: `LEGO_DISABLE_CNAME_SUPPORT: "true"\nHTTPREQ_ENDPOINT: "https://${apiDomain}"\nHTTPREQ_USERNAME: "${username}"\nHTTPREQ_PASSWORD: "${key}"` },
      ],
    }
  }
  const storage = `https://${username}:${key}@${apiDomain}/acmedns`
  return {
    note: 'acme-dns drives the same records and CNAME as httpreq. The storage URL carries your username and API key, so accounts are fetched and domains created automatically; there is no local JSON to pre-seed.',
    snippets: [
      { title: 'Environment variables', code: `ACME_DNS_API_BASE=https://${apiDomain}\nACME_DNS_STORAGE_BASE_URL=${storage}` },
      { title: 'lego', code: `lego --dns acme-dns \\\n  --domains example.com --domains "*.example.com" \\\n  --email admin@example.com --accept-tos run` },
      { title: 'Traefik: traefik.yml', code: `${TRAEFIK_BASE}\n        provider: acme-dns` },
      { title: 'Traefik: docker-compose environment', code: `ACME_DNS_API_BASE: "https://${apiDomain}"\nACME_DNS_STORAGE_BASE_URL: "${storage}"` },
    ],
  }
}
