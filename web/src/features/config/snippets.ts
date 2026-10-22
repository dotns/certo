export type Protocol = 'httpreq' | 'acmedns'
export type ClientId = 'lego' | 'traefik' | 'caddy' | 'acmesh' | 'certmanager'

export interface Snippet {
  /** Translation key of the heading (`clients.snippet.*`). */
  title: string
  code: string
}

export interface ClientSnippets {
  id: ClientId
  title: string
  /** Translation key of the note (`clients.note.*`), interpolated with `client`. */
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
          { title: 'clients.snippet.env', code: env },
          { title: 'clients.snippet.run', code: `lego --dns httpreq --dns.propagation-disable-ans \\\n${LEGO_RUN}` },
        ],
      },
      {
        id: 'traefik',
        title: 'Traefik',
        note: 'clients.note.httpreqTraefik',
        snippets: [
          { title: 'clients.snippet.traefikYml', code: `${TRAEFIK_BASE}\n        provider: httpreq\n        propagation:\n          disableChecks: true` },
          { title: 'clients.snippet.compose', code: `LEGO_DISABLE_CNAME_SUPPORT: "true"\nHTTPREQ_ENDPOINT: "${base}"\nHTTPREQ_USERNAME: "${username}"\nHTTPREQ_PASSWORD: "${key}"` },
        ],
      },
      {
        id: 'caddy',
        title: 'Caddy',
        note: 'clients.note.httpreqUnsupported',
        snippets: [],
      },
      {
        id: 'acmesh',
        title: 'acme.sh',
        note: 'clients.note.httpreqUnsupported',
        snippets: [],
      },
      {
        id: 'certmanager',
        title: 'cert-manager',
        note: 'clients.note.httpreqUnsupported',
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
      note: 'clients.note.acmednsLego',
      snippets: [
        { title: 'clients.snippet.env', code: `ACME_DNS_API_BASE=${base}\nACME_DNS_STORAGE_BASE_URL=${storage}` },
        { title: 'clients.snippet.run', code: `lego --dns acme-dns \\\n${LEGO_RUN}` },
      ],
    },
    {
      id: 'traefik',
      title: 'Traefik',
      note: null,
      snippets: [
        { title: 'clients.snippet.traefikYml', code: `${TRAEFIK_BASE}\n        provider: acme-dns` },
        { title: 'clients.snippet.compose', code: `ACME_DNS_API_BASE: "${base}"\nACME_DNS_STORAGE_BASE_URL: "${storage}"` },
      ],
    },
    {
      id: 'caddy',
      title: 'Caddy',
      note: 'clients.note.acmednsCaddy',
      snippets: [
        { title: 'clients.snippet.caddyfile', code: `example.com {
  tls {
    dns acmedns {
      config /etc/caddy/acmedns.json
    }
  }
}` },
        { title: 'clients.snippet.caddyJson', code: account },
      ],
    },
    {
      id: 'acmesh',
      title: 'acme.sh',
      note: 'clients.note.acmednsAcmesh',
      snippets: [
        { title: 'clients.snippet.env', code: `export ACMEDNS_BASE_URL="${base}"\nexport ACMEDNS_USERNAME="${username}"\nexport ACMEDNS_PASSWORD="${key}"\nexport ACMEDNS_SUBDOMAIN="<subdomain from the Domains page>"` },
        { title: 'clients.snippet.issue', code: `acme.sh --issue --dns dns_acmedns -d example.com -d "*.example.com"` },
      ],
    },
    {
      id: 'certmanager',
      title: 'cert-manager',
      note: 'clients.note.acmednsCertmanager',
      snippets: [
        { title: 'clients.snippet.secret', code: `apiVersion: v1
kind: Secret
metadata:
  name: acme-dns
type: Opaque
stringData:
  acmedns.json: |
${account.split('\n').map(l => `    ${l}`).join('\n')}` },
        { title: 'clients.snippet.issuer', code: `apiVersion: cert-manager.io/v1
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
