// A dedicated server: certo is restarted and the pdns database is wiped in between.
import { describe, test, expect, beforeAll, afterAll } from "bun:test";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { startServer, makeClient, digTxt, digA, dig, uniq, type Server } from "./helpers";

let srv: Server;
beforeAll(async () => { srv = await startServer(); });
afterAll(() => srv?.stop());

function axfr(zone: string, ...flags: string[]): string {
  const r = Bun.spawnSync(["dig", "@127.0.0.1", "-p", String(srv.dnsPort), zone, "AXFR", ...flags]);
  return r.stdout.toString();
}

describe("restart", () => {
  test("challenge TXT and the base zone are rebuilt in pdns from certo's database", async () => {
    const req = makeClient(srv.apiBase);
    const username = uniq("restart");
    const reg = await req("POST", "/api/register", { json: { username, password: "password123" } });
    expect(reg.status).toBe(201);
    const keys = await req("GET", "/api/keys", { token: reg.body.token });
    const apiKey = keys.body[0].key as string;
    const domain = `${uniq("rs")}.test`;
    const add = await req("POST", "/api/domains", { token: reg.body.token, json: { domain } });
    const fqdn = `${add.body.subdomain}.${srv.baseDomain}`;
    const pres = await req("POST", "/present", {
      basic: { user: username, pass: apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "survives-restart" },
    });
    expect(pres.status).toBe(200);
    expect(digTxt(fqdn, srv.dnsPort)).toEqual(["survives-restart"]);

    // Plain restart: both databases persist.
    await srv.restart();
    expect(digTxt(fqdn, srv.dnsPort)).toEqual(["survives-restart"]);
    expect((await req("GET", "/readyz")).status).toBe(200);

    // A hosted zone with records, DNSSEC, a TSIG key and an allow list.
    const adminHdr = { "X-Admin-Key": srv.adminKey };
    const zone = `${uniq("hosted")}.test`;
    expect((await req("POST", "/api/zones", { token: reg.body.token, json: { name: zone } })).status).toBe(201);
    const act = await req("PUT", `/admin/zones/${zone}/activation`, { headers: adminHdr, json: { exempt: true } });
    expect(act.status, act.text).toBe(200);
    const rr = await req("PATCH", `/api/zones/${zone}/rrsets`, {
      token: reg.body.token, json: { rrsets: [{ name: "www", type: "A", records: ["192.0.2.44"] }] },
    });
    expect(rr.status, rr.text).toBe(200);
    const on = await req("PUT", `/api/zones/${zone}/dnssec`, { token: reg.body.token, json: { enabled: true, nsec_mode: "nsec3" } });
    expect(on.status, on.text).toBe(200);
    const key = await req("POST", `/api/zones/${zone}/tsig-keys`, { token: reg.body.token, json: { name: "xfr" } });
    expect(key.status, key.text).toBe(201);
    const allow = await req("PUT", `/api/zones/${zone}/axfr`, { token: reg.body.token, json: { allow_from: ["127.0.0.1"] } });
    expect(allow.status, allow.text).toBe(200);
    expect(digA(`www.${zone}`, srv.dnsPort)).toEqual(["192.0.2.44"]);
    const status = await req("GET", "/admin/sync", { headers: adminHdr });
    expect(status.status).toBe(200);
    expect(status.body.zones).toEqual([]);
    expect(status.body.db_version).toBe(status.body.pdns_version);

    // pdns data lost: everything comes back from certo's database.
    rmSync(join(srv.dir, "pdns", "pdns.sqlite3"), { force: true });
    await srv.restart();
    expect(dig(srv.zone, "SOA", srv.dnsPort).authoritative).toBe(true);
    expect(digTxt(fqdn, srv.dnsPort)).toEqual(["survives-restart"]);
    expect(digA(`www.${zone}`, srv.dnsPort)).toEqual(["192.0.2.44"]);
    const dnssec = await req("GET", `/api/zones/${zone}/dnssec`, { token: reg.body.token });
    expect(dnssec.body).toEqual({ enabled: true, nsec_mode: "nsec3", dnskey: on.body.dnskey, ds: on.body.ds });
    expect(dig(`nope.${zone}`, "A", srv.dnsPort, "+dnssec").authority).toContain("NSEC3");
    const signed = axfr(zone, "-y", `hmac-sha256:xfr:${key.body.secret}`);
    expect(signed).toContain("192.0.2.44");
    const xfr = await req("GET", `/api/zones/${zone}/axfr`, { token: reg.body.token });
    expect(xfr.body).toEqual({ allow_from: ["127.0.0.1/32"], tsig_keys: [{ name: "xfr", algorithm: "hmac-sha256" }] });
    const after = await req("GET", "/admin/sync", { headers: adminHdr });
    expect(after.body.zones).toEqual([]);
    expect(after.body.db_version).toBe(status.body.db_version);
    const clean = await req("POST", "/cleanup", {
      basic: { user: username, pass: apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "survives-restart" },
    });
    expect(clean.status).toBe(200);
    expect(digTxt(fqdn, srv.dnsPort)).toEqual([]);
  });
});
