import { useState, useEffect, useCallback } from 'react';
import type { FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api';
import type { Domain, TXTRecord, APIKeyItem } from '../api';
import { clearToken } from '../auth';

function CopyBtn({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = () => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  return <button className="btn-sm btn-copy" onClick={handleCopy}>{copied ? 'Copied!' : 'Copy'}</button>;
}

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <span className="modal-title">{title}</span>
          <button className="modal-close" onClick={onClose}>&times;</button>
        </div>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  );
}

type Tab = 'domains' | 'keys' | 'config';

export default function Dashboard() {
  const [tab, setTab] = useState<Tab>('domains');
  const [domains, setDomains] = useState<Domain[]>([]);
  const [records, setRecords] = useState<TXTRecord[]>([]);
  const [keys, setKeys] = useState<APIKeyItem[]>([]);
  const [apiDomain, setApiDomain] = useState('');
  const [username, setUsername] = useState('');
  const [appVersion, setAppVersion] = useState('');
  const [newDomain, setNewDomain] = useState('');
  const [newKeyName, setNewKeyName] = useState('');
  const [newKeyScopes, setNewKeyScopes] = useState<string[]>(['']);
  const [showAddDomain, setShowAddDomain] = useState(false);
  const [showAddKey, setShowAddKey] = useState(false);
  const [showScopeHelp, setShowScopeHelp] = useState(false);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const navigate = useNavigate();

  const loadData = useCallback(async () => {
    try {
      const [doms, recs, info, profile, ks] = await Promise.all([
        api.getDomains(), api.getRecords(), api.getInfo(), api.getProfile(), api.getKeys(),
      ]);
      setDomains(doms || []);
      setRecords(recs || []);
      setApiDomain(info.api_domain);
      setAppVersion(info.version || 'dev');
      setUsername(profile.username);
      setKeys(ks || []);
    } catch (err: unknown) {
      if (err instanceof Error && err.message === 'invalid_token') { clearToken(); navigate('/login'); return; }
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally { setLoading(false); }
  }, [navigate]);

  useEffect(() => { loadData(); }, [loadData]);
  useEffect(() => { document.title = 'Dashboard — certo'; }, []);

  const handleAddDomain = async (e: FormEvent) => {
    e.preventDefault(); setError('');
    const domain = newDomain.trim().toLowerCase();
    if (!domain) return;
    try { await api.addDomain(domain); setNewDomain(''); setShowAddDomain(false); await loadData(); }
    catch (err: unknown) { setError(err instanceof Error ? err.message : 'Failed to add domain'); }
  };

  const handleRemoveDomain = async (domain: string) => {
    if (!confirm(`Remove ${domain}?`)) return;
    try { await api.removeDomain(domain); await loadData(); }
    catch (err: unknown) { setError(err instanceof Error ? err.message : 'Failed to remove domain'); }
  };

  const dedupeScopes = (scopes: string[]): string[] => {
    const normalized = scopes.map(s => s.trim().toLowerCase()).filter(Boolean);
    const result: string[] = [];
    for (const s of normalized) {
      const root = s.startsWith('*.') ? s.slice(2) : s;
      const wildcard = '*.' + root;
      // If *.root exists, it covers root — keep only wildcard
      if (normalized.includes(wildcard)) {
        if (!result.includes(wildcard)) result.push(wildcard);
      } else {
        if (!result.includes(s)) result.push(s);
      }
    }
    return result;
  };

  const handleCreateKey = async (e: FormEvent) => {
    e.preventDefault(); setError('');
    if (!newKeyName.trim()) return;
    const scope = dedupeScopes(newKeyScopes);
    try {
      await api.createKey(newKeyName.trim(), scope.length > 0 ? scope : ['*']);
      setNewKeyName(''); setNewKeyScopes(['']); setShowAddKey(false); await loadData();
    }
    catch (err: unknown) { setError(err instanceof Error ? err.message : 'Failed to create key'); }
  };

  const handleDeleteKey = async (id: number, name: string) => {
    if (!confirm(`Delete key "${name}"?`)) return;
    try { await api.deleteKey(id); await loadData(); }
    catch (err: unknown) { setError(err instanceof Error ? err.message : 'Failed to delete key'); }
  };

  if (loading) return <div className="loading">Loading...</div>;

  return (
    <div className="dashboard">
      <div className="topbar-brand">
        <div className="topbar-brand-name">CERTO</div>
        <div className="topbar-brand-tagline">ACME httpreq Server</div>
        <button className="topbar-logout" onClick={() => { clearToken(); navigate('/login'); }}>{username} / Logout</button>
      </div>
      <div className="topbar">
        <div className="topbar-nav">
          <button className={`nav-item ${tab === 'domains' ? 'active' : ''}`} onClick={() => setTab('domains')}>
            DOMAINS <span className="nav-count">{domains.length}</span>
          </button>
          <button className={`nav-item ${tab === 'keys' ? 'active' : ''}`} onClick={() => setTab('keys')}>
            API KEYS <span className="nav-count">{keys.length}</span>
          </button>
          <button className={`nav-item ${tab === 'config' ? 'active' : ''}`} onClick={() => setTab('config')}>
            CONFIG
          </button>
        </div>
        <div className="topbar-actions">
          {tab === 'domains' && <button className="btn-action" onClick={() => setShowAddDomain(true)}>+ Domain</button>}
          {tab === 'keys' && <button className="btn-action" onClick={() => setShowAddKey(true)}>+ Key</button>}
        </div>
      </div>

      {error && <div className="error">{error}</div>}

      {/* Add Domain Modal */}
      {showAddDomain && (
        <Modal title="Add Domain" onClose={() => setShowAddDomain(false)}>
          <form className="modal-form" onSubmit={handleAddDomain}>
            <input type="text" placeholder="example.com" value={newDomain}
              onChange={(e) => setNewDomain(e.target.value)} autoFocus />
            <button type="submit" className="btn-primary">Add Domain</button>
          </form>
        </Modal>
      )}

      {/* Add Key Modal */}
      {showAddKey && (
        <Modal title="Create API Key" onClose={() => { setShowAddKey(false); setNewKeyScopes(['']); }}>
          <form className="modal-form" onSubmit={handleCreateKey}>
            <input type="text" placeholder="Key name" value={newKeyName}
              onChange={(e) => setNewKeyName(e.target.value)} autoFocus />
            <div className="scope-label">
              Domains
              <span className="scope-help" onClick={() => setShowScopeHelp(!showScopeHelp)}>?</span>
              <span className="scope-hint">Leave empty for global access</span>
            </div>
            {showScopeHelp && (
              <div className="scope-help-box">
                <strong>Define which domains this key can access:</strong>
                <ul>
                  <li><code>*.example.com</code> — example.com and all subdomains</li>
                  <li><code>example.com</code> — exact domain only</li>
                  <li>Leave all empty — global access (all domains)</li>
                </ul>
                <p>*.example.com already covers example.com, duplicates are removed automatically.</p>
              </div>
            )}
            <div className="scope-list">
              {newKeyScopes.map((s, i) => (
                <div className="scope-row" key={i}>
                  <input type="text" placeholder="*.example.com" value={s}
                    onChange={(e) => { const arr = [...newKeyScopes]; arr[i] = e.target.value; setNewKeyScopes(arr); }} />
                  {newKeyScopes.length > 1 && (
                    <button type="button" className="scope-remove" onClick={() => setNewKeyScopes(newKeyScopes.filter((_, j) => j !== i))}>×</button>
                  )}
                </div>
              ))}
              <button type="button" className="scope-add" onClick={() => setNewKeyScopes([...newKeyScopes, ''])}>+ Add domain</button>
            </div>
            <button type="submit" className="btn-primary">Create Key</button>
          </form>
        </Modal>
      )}

      {/* Domains Tab */}
      {tab === 'domains' && (
        <div className="card tab-card">
          <table>
            <thead>
              <tr><th>Domain</th><th>CNAME Name</th><th>CNAME Value</th><th></th></tr>
            </thead>
            <tbody>
              {domains.length === 0 ? (
                <tr><td colSpan={4} className="empty">No domains added yet</td></tr>
              ) : (
                domains.map((d) => {
                  const name = `_acme-challenge.${d.domain}`;
                  return (
                    <tr key={d.domain}>
                      <td>{d.domain}</td>
                      <td><code>{name}</code> <CopyBtn text={name} /></td>
                      <td><code>{d.cname_target}</code> <CopyBtn text={d.cname_target} /></td>
                      <td><button className="btn-sm btn-delete" onClick={() => handleRemoveDomain(d.domain)}>Delete</button></td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
          {records.length > 0 && (
            <>
              <div className="card-divider" />
              <div className="card-header"><span className="card-title">Active TXT Records</span></div>
              <table>
                <thead><tr><th>Domain</th><th>Value</th><th>Updated</th></tr></thead>
                <tbody>
                  {records.map((r, i) => (
                    <tr key={i}>
                      <td>{r.domain}</td>
                      <td className="mono">{r.value}</td>
                      <td>{new Date(r.last_update).toLocaleString()}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>
      )}

      {/* API Keys Tab */}
      {tab === 'keys' && (
        <div className="card tab-card">
          <table>
            <thead><tr><th>Name</th><th>Key</th><th>Domains</th><th></th></tr></thead>
            <tbody>
              {keys.length === 0 ? (
                <tr><td colSpan={4} className="empty">No API keys</td></tr>
              ) : (
                keys.map((k) => (
                  <tr key={k.id}>
                    <td>{k.name}</td>
                    <td><code>{k.key}</code> <CopyBtn text={k.key} /></td>
                    <td>{k.scope.includes('*') ? <em>Global</em> : <code>{k.scope.join(', ')}</code>}</td>
                    <td><button className="btn-sm btn-delete" onClick={() => handleDeleteKey(k.id, k.name)}>Delete</button></td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      )}

      {/* Config Tab */}
      {tab === 'config' && (() => {
        const legoEnv = `HTTPREQ_ENDPOINT=https://${apiDomain}\nHTTPREQ_USERNAME=${username}\nHTTPREQ_PASSWORD=<your-api-key>\nLEGO_DISABLE_CNAME_SUPPORT=true`;
        const legoCmd = `lego --dns httpreq --dns.propagation-disable-ans \\\n  --domains example.com --domains "*.example.com" \\\n  --email admin@example.com --accept-tos run`;
        const traefikYaml = `certificatesResolvers:\n  letsencrypt:\n    acme:\n      email: admin@example.com\n      storage: /data/ssl/acme.json\n      dnsChallenge:\n        provider: httpreq\n        propagation:\n          disableChecks: true`;
        const dockerEnv = `LEGO_DISABLE_CNAME_SUPPORT: "true"\nHTTPREQ_ENDPOINT: "https://${apiDomain}"\nHTTPREQ_USERNAME: "${username}"\nHTTPREQ_PASSWORD: "<your-api-key>"`;
        return (
          <div className="card tab-card">
            <div className="config-sections">
              <div className="config-section">
                <div className="config-label">Environment Variables <CopyBtn text={legoEnv} /></div>
                <pre className="config-pre">{legoEnv}</pre>
              </div>
              <div className="config-section">
                <div className="config-label">lego Command <CopyBtn text={legoCmd} /></div>
                <pre className="config-pre">{legoCmd}</pre>
              </div>
              <div className="config-section">
                <div className="config-label">Traefik — traefik.yml <CopyBtn text={traefikYaml} /></div>
                <pre className="config-pre">{traefikYaml}</pre>
              </div>
              <div className="config-section">
                <div className="config-label">Traefik — docker-compose environment <CopyBtn text={dockerEnv} /></div>
                <pre className="config-pre">{dockerEnv}</pre>
              </div>
            </div>
          </div>
        );
      })()}

      <div className="dash-footer">
        <a href="https://github.com/dotns/certo" target="_blank" rel="noreferrer">GitHub</a>
        <span>&middot;</span>
        <a href="/llms.txt" target="_blank">llms.txt</a>
        <span>&middot;</span>
        <span>{appVersion}</span>
      </div>
    </div>
  );
}
