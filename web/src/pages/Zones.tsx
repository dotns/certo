import { useState, useEffect, useCallback } from 'react';
import type { FormEvent } from 'react';
import { api } from '../api';
import type { Zone, ZoneRecord, Dnssec, Transfer, TsigKey } from '../api';
import { CopyBtn, Modal } from '../ui';

const TYPES = ['A', 'AAAA', 'CNAME', 'MX', 'TXT', 'NS', 'PTR', 'SRV', 'CAA'];
const EXAMPLES: Record<string, string> = {
  A: '192.0.2.1',
  AAAA: '2001:db8::1',
  CNAME: 'target.example.com',
  MX: '10 mail.example.com',
  TXT: '"v=spf1 -all"',
  NS: 'ns1.example.net',
  PTR: 'host.example.com',
  SRV: '10 5 443 target.example.com',
  CAA: '0 issue "letsencrypt.org"',
};
const TSIG_ALGORITHMS = ['hmac-sha256', 'hmac-sha384', 'hmac-sha512'];

const message = (err: unknown, fallback: string) => (err instanceof Error ? err.message : fallback);
const fetchZone = (zone: string) =>
  Promise.all([api.getZone(zone), api.getDnssec(zone), api.getTransfer(zone)]);
const isReverse = (zone: string) => /\.(in-addr|ip6)\.arpa$/.test(zone);

interface ListProps {
  zones: Zone[];
  onOpen: (zone: string) => void;
  onDelete: (zone: string) => void;
}

export function ZoneList({ zones, onOpen, onDelete }: ListProps) {
  return (
    <div className="card tab-card">
      <table>
        <thead><tr><th>Zone</th><th>Serial</th><th>DNSSEC</th><th></th></tr></thead>
        <tbody>
          {zones.length === 0 ? (
            <tr><td colSpan={4} className="empty">No zones yet</td></tr>
          ) : (
            zones.map((z) => (
              <tr key={z.name}>
                <td><button className="link-btn" onClick={() => onOpen(z.name)}>{z.name}</button></td>
                <td className="mono">{z.serial}</td>
                <td>{z.dnssec ? <span className="badge badge-on">Signed</span> : <span className="badge">Off</span>}</td>
                <td className="actions">
                  <button className="btn-sm btn-copy" onClick={() => onOpen(z.name)}>Manage</button>
                  <button className="btn-sm btn-delete" onClick={() => onDelete(z.name)}>Delete</button>
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}

interface DetailProps {
  zone: string;
  nsName: string;
  onBack: () => void;
  onChanged: () => void;
  onError: (msg: string) => void;
}

export function ZoneDetail({ zone, nsName, onBack, onChanged, onError }: DetailProps) {
  const defaultType = isReverse(zone) ? 'PTR' : 'A';
  const [records, setRecords] = useState<ZoneRecord[]>([]);
  const [dnssec, setDnssec] = useState<Dnssec | null>(null);
  const [transfer, setTransfer] = useState<Transfer | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [form, setForm] = useState({ name: '', type: defaultType, ttl: '', data: '' });
  const [nsecMode, setNsecMode] = useState('nsec');
  const [allowFrom, setAllowFrom] = useState('');
  const [keyName, setKeyName] = useState('');
  const [keyAlg, setKeyAlg] = useState(TSIG_ALGORITHMS[0]);
  const [created, setCreated] = useState<TsigKey | null>(null);

  const apply = useCallback(([z, d, t]: [Zone, Dnssec, Transfer]) => {
    setRecords(z.records || []);
    setDnssec(d);
    setNsecMode(d.nsec_mode);
    setTransfer(t);
    setAllowFrom(t.allow_from.join('\n'));
  }, []);

  const load = async () => apply(await fetchZone(zone));

  useEffect(() => {
    let active = true;
    fetchZone(zone).then(
      (data) => { if (active) apply(data); },
      (err: unknown) => onError(message(err, 'Failed to load zone')),
    );
    return () => { active = false; };
  }, [zone, apply, onError]);

  const run = async (action: () => Promise<unknown>, fallback: string) => {
    onError('');
    try { await action(); await load(); onChanged(); return true; }
    catch (err: unknown) { onError(message(err, fallback)); return false; }
  };

  const resetForm = () => { setEditing(null); setForm({ name: '', type: defaultType, ttl: '', data: '' }); };

  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    const rec = {
      name: form.name.trim() || '@',
      type: form.type,
      data: form.data.trim(),
      ...(form.ttl.trim() ? { ttl: Number(form.ttl) } : {}),
    };
    const ok = await run(
      () => (editing === null ? api.addRecord(zone, rec) : api.updateRecord(zone, editing, rec)),
      'Failed to save record',
    );
    if (ok) resetForm();
  };

  const startEdit = (r: ZoneRecord) => {
    const suffix = `.${zone}`;
    const name = r.name === zone ? '@' : r.name.endsWith(suffix) ? r.name.slice(0, -suffix.length) : r.name;
    setEditing(r.id);
    setForm({ name, type: r.type, ttl: String(r.ttl), data: r.data });
  };

  const handleDeleteRecord = (r: ZoneRecord) => {
    if (!confirm(`Delete ${r.type} record ${r.name}?`)) return;
    run(() => api.deleteRecord(zone, r.id), 'Failed to delete record');
  };

  const handleDnssec = (enabled: boolean) => {
    if (!enabled && !confirm('Disable DNSSEC? Remove the DS record at the parent first, or the zone will fail validation.')) return;
    run(() => api.setDnssec(zone, enabled, nsecMode), 'Failed to change DNSSEC');
  };

  const handleAllowFrom = (e: FormEvent) => {
    e.preventDefault();
    const list = allowFrom.split(/[\s,]+/).filter(Boolean);
    run(() => api.setTransfer(zone, list), 'Failed to save transfer settings');
  };

  const handleAddKey = async (e: FormEvent) => {
    e.preventDefault();
    if (!keyName.trim()) return;
    let key: TsigKey | null = null;
    const ok = await run(async () => { key = await api.addTsigKey(zone, keyName.trim(), keyAlg); }, 'Failed to create key');
    if (ok) { setKeyName(''); setCreated(key); }
  };

  const handleDeleteKey = (name: string) => {
    if (!confirm(`Delete TSIG key ${name}?`)) return;
    run(() => api.deleteTsigKey(zone, name), 'Failed to delete key');
  };

  return (
    <>
      {created?.secret && (
        <Modal title="TSIG key created" onClose={() => setCreated(null)}>
          <div className="modal-form">
            <p className="scope-hint">Copy the secret now; it is not shown again.</p>
            <div className="cred-row"><span className="cred-label">Name</span><span className="cred-value">{created.name}</span></div>
            <div className="cred-row"><span className="cred-label">Algorithm</span><span className="cred-value">{created.algorithm}</span></div>
            <div className="cred-row">
              <span className="cred-label">Secret</span><span className="cred-value">{created.secret}</span>
              <CopyBtn text={created.secret} />
            </div>
            <pre className="config-pre wrap">{`dig @${nsName} ${zone} AXFR -y ${created.algorithm}:${created.name}:${created.secret}`}</pre>
          </div>
        </Modal>
      )}

      <div className="card tab-card">
        <div className="card-header">
          <span className="card-title"><button className="link-btn" onClick={onBack}>Zones</button> / <span className="zone-name">{zone}</span></span>
        </div>
        <form className="add-form" onSubmit={handleSave}>
          <input className="narrow" type="text" placeholder="@" value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })} />
          <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value })}>
            {TYPES.map((t) => <option key={t}>{t}</option>)}
          </select>
          <input className="narrow" type="number" min={0} placeholder="TTL 3600" value={form.ttl}
            onChange={(e) => setForm({ ...form, ttl: e.target.value })} />
          <input type="text" placeholder={EXAMPLES[form.type]} value={form.data}
            onChange={(e) => setForm({ ...form, data: e.target.value })} />
          <button type="submit">{editing === null ? 'Add' : 'Save'}</button>
          {editing !== null && <button type="button" className="btn-secondary" onClick={resetForm}>Cancel</button>}
        </form>
        <table>
          <thead><tr><th>Name</th><th>Type</th><th>TTL</th><th>Data</th><th></th></tr></thead>
          <tbody>
            {records.length === 0 ? (
              <tr><td colSpan={5} className="empty">No records</td></tr>
            ) : (
              records.map((r) => (
                <tr key={r.id} className={editing === r.id ? 'editing' : ''}>
                  <td>{r.name}</td>
                  <td>{r.type}</td>
                  <td className="mono">{r.ttl}</td>
                  <td className="mono">{r.data}</td>
                  <td className="actions">
                    <button className="btn-sm btn-copy" onClick={() => startEdit(r)}>Edit</button>
                    <button className="btn-sm btn-delete" onClick={() => handleDeleteRecord(r)}>Delete</button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {dnssec && (
        <div className="card">
          <div className="card-header">
            <span className="card-title">DNSSEC</span>
            {dnssec.enabled ? <span className="badge badge-on">Signed ({dnssec.nsec_mode.toUpperCase()})</span> : <span className="badge">Off</span>}
          </div>
          <div className="config-sections">
            <div className="config-section">
              <div className="config-label">
                <select value={nsecMode} onChange={(e) => setNsecMode(e.target.value)}>
                  <option value="nsec">NSEC</option>
                  <option value="nsec3">NSEC3</option>
                </select>
                {dnssec.enabled ? (
                  <>
                    {nsecMode !== dnssec.nsec_mode && <button className="btn-sm btn-copy" onClick={() => handleDnssec(true)}>Apply</button>}
                    <button className="btn-sm btn-delete" onClick={() => handleDnssec(false)}>Disable</button>
                  </>
                ) : (
                  <button className="btn-sm btn-copy" onClick={() => handleDnssec(true)}>Enable</button>
                )}
              </div>
            </div>
            {dnssec.ds && (
              <div className="config-section">
                <div className="config-label">DS record for the parent zone <CopyBtn text={`${zone}. IN DS ${dnssec.ds}`} /></div>
                <pre className="config-pre">{`${zone}. IN DS ${dnssec.ds}`}</pre>
              </div>
            )}
            {dnssec.dnskey && (
              <div className="config-section">
                <div className="config-label">DNSKEY <CopyBtn text={`${zone}. IN DNSKEY ${dnssec.dnskey}`} /></div>
                <pre className="config-pre wrap">{`${zone}. IN DNSKEY ${dnssec.dnskey}`}</pre>
              </div>
            )}
          </div>
        </div>
      )}

      {transfer && (
        <div className="card">
          <div className="card-header"><span className="card-title">Zone transfer (AXFR)</span></div>
          <div className="config-sections">
            <form className="config-section" onSubmit={handleAllowFrom}>
              <div className="config-label">Allowed source addresses <span className="scope-hint">IPs or CIDRs; empty denies unsigned transfers</span></div>
              <div className="add-form flush">
                <textarea rows={2} placeholder="192.0.2.53, 2001:db8::/64" value={allowFrom}
                  onChange={(e) => setAllowFrom(e.target.value)} />
                <button type="submit">Save</button>
              </div>
            </form>
            <div className="config-section">
              <div className="config-label">TSIG keys</div>
              <form className="add-form flush" onSubmit={handleAddKey}>
                <input type="text" placeholder={`xfr.${zone}`} value={keyName} onChange={(e) => setKeyName(e.target.value)} />
                <select value={keyAlg} onChange={(e) => setKeyAlg(e.target.value)}>
                  {TSIG_ALGORITHMS.map((a) => <option key={a}>{a}</option>)}
                </select>
                <button type="submit">Create</button>
              </form>
            </div>
          </div>
          <table>
            <thead><tr><th>Key name</th><th>Algorithm</th><th>Created</th><th></th></tr></thead>
            <tbody>
              {transfer.tsig_keys.length === 0 ? (
                <tr><td colSpan={4} className="empty">No TSIG keys</td></tr>
              ) : (
                transfer.tsig_keys.map((k) => (
                  <tr key={k.name}>
                    <td><code>{k.name}</code></td>
                    <td>{k.algorithm}</td>
                    <td>{new Date(k.created_at * 1000).toLocaleString()}</td>
                    <td><button className="btn-sm btn-delete" onClick={() => handleDeleteKey(k.name)}>Delete</button></td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
