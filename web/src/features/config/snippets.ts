export type Protocol = 'httpreq' | 'acmedns'
export type ClientId = 'lego' | 'traefik' | 'caddy' | 'acmesh' | 'certmanager'

export interface Snippet {
  title: string
  code: string
}

export interface ClientSnippets {
  id: ClientId
  title: string
  note: string | null
  snippets: Snippet[]
}

export const CLIENTS: ReadonlyArray<{ id: ClientId, title: string }> = [
  { id: 'lego', title: 'lego' },
  { id: 'traefik', title: 'Traefik' },
  { id: 'caddy', title: 'Caddy' },
  { id: 'acmesh', title: 'acme.sh' },
  { id: 'certmanager', title: 'cert-manager' },
]

/** Placeholder shown when no key is selected. */
export const KEY_PLACEHOLDER = '<your-api-key>'

const TRAEFIK_BASE = `certificatesResolvers:
  letsencrypt:
    acme:
      email: admin@example.com
      storage: /data/ssl/acme.json
      dnsChallenge:`

const LEGO_RUN = `  --domains example.com --domains "*.example.com" \\
  --email admin@example.com --accept-tos run`

/** Configuration examples for every supported client under `protocol`. */
export function clientSnippets(protocol: Protocol, apiDomain: string, username: string, key = KEY_PLACEHOLDER): ClientSnippets[] {
  const base = `https://${apiDomain}`
  if (protocol === 'httpreq') {
    const env = `HTTPREQ_ENDPOINT=${base}\nHTTPREQ_USERNAME=${username}\nHTTPREQ_PASSWORD=${key}\nLEGO_DISABLE_CNAME_SUPPORT=true`
    return [
      {
        id: 'lego',
        title: 'lego',
        note: null,
        snippets: [
          { title: 'Environment variables', code: env },
          { title: 'Run', code: `lego --dns httpreq --dns.propagation-disable-ans \\\n${LEGO_RUN}` },
        ],
      },
      {
        id: 'traefik',
        title: 'Traefik',
        note: 'Traefik embeds lego; the resolver uses the httpreq provider with the same variables.',
        snippets: [
          { title: 'traefik.yml', code: `${TRAEFIK_BASE}\n        provider: httpreq\n        propagation:\n          disableChecks: true` },
          { title: 'docker-compose environment', code: `LEGO_DISABLE_CNAME_SUPPORT: "true"\nHTTPREQ_ENDPOINT: "${base}"\nHTTPREQ_USERNAME: "${username}"\nHTTPREQ_PASSWORD: "${key}"` },
        ],
      },
      {
        id: 'caddy',
        title: 'Caddy',
        note: 'Caddy has no httpreq module; use the acme-dns protocol instead.',
        snippets: [],
      },
      {
        id: 'acmesh',
        title: 'acme.sh',
        note: 'acme.sh has no httpreq provider; use the acme-dns protocol instead.',
        snippets: [],
      },
      {
        id: 'certmanager',
        title: 'cert-manager',
        note: 'cert-manager has no httpreq solver; use the acme-dns protocol instead.',
        snippets: [],
      },
    ]
  }
  const storage = `https://${username}:${key}@${apiDomain}/acmedns`
  const account = `{
  "example.com": {
    "username": "${username}",
    "password": "${key}",
    "fulldomain": "<cname target from the Domains page>",
    "subdomain": "<subdomain from the Domains page>",
    "allowfrom": []
  }
}`
  return [
    {
      id: 'lego',
      title: 'lego',
      note: 'The storage URL carries your username and API key, so accounts are fetched and domains created automatically; there is no local JSON to pre-seed.',
      snippets: [
        { title: 'Environment variables', code: `ACME_DNS_API_BASE=${base}\nACME_DNS_STORAGE_BASE_URL=${storage}` },
        { title: 'Run', code: `lego --dns acme-dns \\\n${LEGO_RUN}` },
      ],
    },
    {
      id: 'traefik',
      title: 'Traefik',
      note: null,
      snippets: [
        { title: 'traefik.yml', code: `${TRAEFIK_BASE}\n        provider: acme-dns` },
        { title: 'docker-compose environment', code: `ACME_DNS_API_BASE: "${base}"\nACME_DNS_STORAGE_BASE_URL: "${storage}"` },
      ],
    },
    {
      id: 'caddy',
      title: 'Caddy',
      note: 'Build Caddy with the acmedns module (xcaddy build --with github.com/caddy-dns/acmedns) and reference the account file.',
      snippets: [
        { title: 'Caddyfile', code: `example.com {
  tls {
    dns acmedns {
      config /etc/caddy/acmedns.json
    }
  }
}` },
        { title: '/etc/caddy/acmedns.json', code: account },
      ],
    },
    {
      id: 'acmesh',
      title: 'acme.sh',
      note: 'Set the account details once; acme.sh stores them in its config for renewals.',
      snippets: [
        { title: 'Environment variables', code: `export ACMEDNS_BASE_URL="${base}"\nexport ACMEDNS_USERNAME="${username}"\nexport ACMEDNS_PASSWORD="${key}"\nexport ACMEDNS_SUBDOMAIN="<subdomain from the Domains page>"` },
        { title: 'Issue', code: `acme.sh --issue --dns dns_acmedns -d example.com -d "*.example.com"` },
      ],
    },
    {
      id: 'certmanager',
      title: 'cert-manager',
      note: 'The Secret holds the acme-dns account JSON; one entry per domain.',
      snippets: [
        { title: 'Secret', code: `apiVersion: v1
kind: Secret
metadata:
  name: acme-dns
type: Opaque
stringData:
  acmedns.json: |
${account.split('\n').map(l => `    ${l}`).join('\n')}` },
        { title: 'Issuer', code: `apiVersion: cert-manager.io/v1
kind: Issuer
metadata:
  name: letsencrypt
spec:
  acme:
    server: https://acme-v02.api.letsencrypt.org/directory
    email: admin@example.com
    privateKeySecretRef:
      name: letsencrypt-account
    solvers:
      - dns01:
          acmeDNS:
            host: ${base}
            accountSecretRef:
              name: acme-dns
              key: acmedns.json` },
      ],
    },
  ]
}
