import { describe, test, expect, beforeAll } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ensureServer, req, newUser, dig, uniq, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

const adminHdr = () => ({ "X-Admin-Key": srv.adminKey });

/** Creates a zone; it starts pending because no parent delegates it. */
async function newZone(token: string, name: string) {
  const r = await req("POST", "/api/zones", { token, json: { name } });
  expect(r.status, r.text).toBe(201);
  expect(r.body.status).toBe("pending");
  return r.body;
}

/** Creates a zone and has the admin exempt it from the delegation check so it is served. */
async function activeZone(token: string, name: string) {
  const zone = await newZone(token, name);
  const r = await req("PUT", `/admin/zones/${zone.name}/activation`, { headers: adminHdr(), json: { exempt: true } });
  expect(r.status, r.text).toBe(200);
  expect(r.body.status).toBe("active");
  expect(r.body.exempt).toBe(true);
  return r.body;
}

async function patch(token: string, zone: string, rrsets: Record<string, unknown>[]) {
  const r = await req("PATCH", `/api/zones/${zone}/rrsets`, { token, json: { rrsets } });
  expect(r.status, r.text).toBe(200);
  return r.body as { name: string; type: string; ttl: number; records: string[] }[];
}

describe("hosted zones", () => {
  test("pending until delegated or exempted; verify reports the resolver failure", async () => {
    const u = await newUser();
    const name = `${uniq("pend")}.test`;
    const zone = await newZone(u.token, name);
    expect(zone.ns_target).toBe(srv.baseDomain);
    expect(zone.exempt).toBe(false);
    expect(dig(name, "SOA", srv.dnsPort).status).toBe("REFUSED");

    const blocked = await req("PATCH", `/api/zones/${name}/rrsets`, {
      token: u.token, json: { rrsets: [{ name: "www", type: "A", records: ["192.0.2.1"] }] },
    });
    expect(blocked.status).toBe(409);
    expect(blocked.body.error).toBe("zone_not_active");

    // The e2e resolver is pdns itself, which refuses: the check is inconclusive.
    const verify = await req("POST", `/api/zones/${name}/verify`, { token: u.token });
    expect(verify.status).toBe(503);
    expect(verify.body.error).toBe("check_failed");

    const own = await req("PUT", `/api/zones/${name}/activation`, { token: u.token, json: { exempt: true } });
    expect(own.status).toBe(404);
    const active = await req("PUT", `/admin/zones/${name}/activation`, { headers: adminHdr(), json: { exempt: true } });
    expect(active.body).toMatchObject({ name, status: "active", exempt: true, owner: u.username });
    expect(dig(name, "SOA", srv.dnsPort).authoritative).toBe(true);
    expect(dig(name, "NS", srv.dnsPort).answers).toEqual([`NS ${srv.baseDomain}.`]);

    // Lifting the exemption keeps the zone active until the daily checks fail.
    const normal = await req("PUT", `/admin/zones/${name}/activation`, { headers: adminHdr(), json: { exempt: false } });
    expect(normal.body).toMatchObject({ status: "active", exempt: false });
    const base = await req("PUT", `/admin/zones/${srv.baseDomain}/activation`, { headers: adminHdr(), json: { exempt: false } });
    expect(base.status).toBe(403);
  });

  test("reverse zone serves PTR (dig -x), NXDOMAIN with SOA, apex SOA/NS", async () => {
    const u = await newUser();
    const zone = await activeZone(u.token, "23.251.162.in-addr.arpa.");
    expect(zone.name).toBe("23.251.162.in-addr.arpa");
    await patch(u.token, zone.name, [{ name: "5", type: "PTR", records: ["host.example.com."] }]);

    const ptr = dig("", "", srv.dnsPort, "-x", "162.251.23.5");
    expect(ptr.status).toBe("NOERROR");
    expect(ptr.authoritative).toBe(true);
    expect(ptr.answers).toEqual(["PTR host.example.com."]);

    const missing = dig("", "", srv.dnsPort, "-x", "162.251.23.6");
    expect(missing.status).toBe("NXDOMAIN");
    expect(missing.authority).toEqual(["SOA"]);

    const soa = dig(zone.name, "SOA", srv.dnsPort);
    expect(soa.authoritative).toBe(true);
    expect(soa.answers[0]).toStartWith(`SOA ${srv.baseDomain}.`);
    expect(dig(zone.name, "NS", srv.dnsPort).answers).toEqual([`NS ${srv.baseDomain}.`]);
  });

  test("forward zone: rrsets, NODATA, CNAME, replace and delete", async () => {
    const u = await newUser();
    const name = `${uniq("fwd")}.test`;
    await activeZone(u.token, name);
    const after = await patch(u.token, name, [
      { name: "www", type: "A", ttl: 120, records: ["192.0.2.10"] },
      { name: "alias", type: "CNAME", records: [`www.${name}.`] },
      { name: "@", type: "MX", records: [`10 mail.${name}.`] },
      { name: "@", type: "TXT", records: ["v=spf1 -all"] },
    ]);
    expect(after.find((r) => r.type === "A")).toEqual({ name: `www.${name}.`, type: "A", ttl: 120, records: ["192.0.2.10"] });
    expect(after.find((r) => r.type === "TXT")?.records).toEqual(['"v=spf1 -all"']);
    const listed = await req("GET", `/api/zones/${name}/rrsets`, { token: u.token });
    expect(listed.body.map((r: any) => r.type).sort()).toEqual(["A", "CNAME", "MX", "NS", "SOA", "TXT"]);
    const detail = await req("GET", `/api/zones/${name}`, { token: u.token });
    expect(detail.body.rrsets).toHaveLength(6);

    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual(["A 192.0.2.10"]);
    expect(dig(`alias.${name}`, "A", srv.dnsPort).answers).toEqual([`CNAME www.${name}.`, "A 192.0.2.10"]);
    expect(dig(name, "MX", srv.dnsPort).answers).toEqual([`MX 10 mail.${name}.`]);
    expect(dig(name, "TXT", srv.dnsPort).answers).toEqual(['TXT "v=spf1 -all"']);

    const nodata = dig(`www.${name}`, "AAAA", srv.dnsPort);
    expect(nodata.status).toBe("NOERROR");
    expect(nodata.answers).toEqual([]);
    expect(nodata.authority).toEqual(["SOA"]);

    const apexCname = await req("PATCH", `/api/zones/${name}/rrsets`, {
      token: u.token, json: { rrsets: [{ name: "@", type: "CNAME", records: ["elsewhere.test."] }] },
    });
    expect(apexCname.status).toBe(400);
    expect(apexCname.body.error).toBe("cname_at_apex");
    const ptr = await req("PATCH", `/api/zones/${name}/rrsets`, {
      token: u.token, json: { rrsets: [{ name: "x", type: "PTR", records: ["h.test."] }] },
    });
    expect(ptr.body.error).toBe("ptr_outside_reverse_zone");
    const bad = await req("PATCH", `/api/zones/${name}/rrsets`, {
      token: u.token, json: { rrsets: [{ name: "www", type: "A", records: ["not-an-address"] }] },
    });
    expect(bad.status).toBe(400);
    expect(bad.body.error).toBe("pdns_rejected");

    await patch(u.token, name, [{ name: "www", type: "A", records: ["192.0.2.11"] }]);
    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual(["A 192.0.2.11"]);

    const gone = await patch(u.token, name, [{ name: "www", type: "A", changetype: "DELETE" }]);
    expect(gone.some((r) => r.type === "A")).toBe(false);
    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual([]);

    expect((await req("DELETE", `/api/zones/${name}`, { token: u.token })).status).toBe(204);
    expect(dig(name, "SOA", srv.dnsPort).status).toBe("REFUSED");
    expect((await req("GET", `/api/zones/${name}`, { token: u.token })).status).toBe(404);
  });

  test("ownership, scope and collisions", async () => {
    const owner = await newUser();
    const other = await newUser();
    const name = `${uniq("own")}.test`;
    await newZone(owner.token, name);

    expect((await req("GET", `/api/zones/${name}`, { token: other.token })).status).toBe(404);
    expect((await req("DELETE", `/api/zones/${name}`, { token: other.token })).status).toBe(404);
    const dup = await req("POST", "/api/zones", { token: other.token, json: { name } });
    expect(dup.status).toBe(409);
    expect(dup.body.error).toBe("zone_already_exists");
    const child = await req("POST", "/api/zones", { token: other.token, json: { name: `sub.${name}` } });
    expect(child.status).toBe(403);
    expect(child.body.error).toBe("zone_not_allowed");
    const own = await req("POST", "/api/zones", { token: owner.token, json: { name: `x.${srv.baseDomain}` } });
    expect(own.body.error).toBe("zone_not_allowed");
    expect((await req("GET", `/api/zones/${srv.baseDomain}`, { token: owner.token })).status).toBe(404);

    const key = await req("POST", "/api/keys", { token: owner.token, json: { name: "z", scope: ["*.elsewhere.test"] } });
    const scoped = await req("GET", `/api/zones/${name}`, { token: key.body.key });
    expect(scoped.status).toBe(403);
    expect(scoped.body.error).toBe("domain_not_in_scope");
  });

  test("admin creates zones for a user and sees the base zone", async () => {
    const u = await newUser();
    const users = await req("GET", "/admin/users", { headers: adminHdr() });
    const uid = users.body.find((x: any) => x.username === u.username).id;
    const name = `${uniq("adm")}.test`;
    const created = await req("POST", "/admin/zones", { headers: adminHdr(), json: { user_id: uid, name } });
    expect(created.status).toBe(201);
    expect(created.body).toMatchObject({ name, owner: u.username, status: "pending", system: false });
    const mine = await req("GET", "/api/zones", { token: u.token });
    expect(mine.body.map((z: any) => z.name)).toContain(name);

    const all = await req("GET", "/admin/zones", { headers: adminHdr() });
    const base = all.body.find((z: any) => z.system);
    expect(base).toMatchObject({ name: srv.baseDomain, status: "active", exempt: true });
    expect(base.owner).toBeUndefined();
    expect((await req("DELETE", `/admin/zones/${srv.baseDomain}`, { headers: adminHdr() })).status).toBe(403);

    // Deleting the account unpublishes its zones.
    await req("PUT", `/admin/zones/${name}/activation`, { headers: adminHdr(), json: { exempt: true } });
    expect(dig(name, "SOA", srv.dnsPort).authoritative).toBe(true);
    expect((await req("DELETE", "/api/profile", { token: u.token })).status).toBe(204);
    expect(dig(name, "SOA", srv.dnsPort).status).toBe("REFUSED");
    expect((await req("GET", "/admin/zones", { headers: adminHdr() })).body.map((z: any) => z.name)).not.toContain(name);
  });

  test("DNSSEC signs answers and the transferred zone verifies", async () => {
    const u = await newUser();
    const name = `${uniq("sec")}.test`;
    await activeZone(u.token, name);
    await patch(u.token, name, [{ name: "www", type: "A", records: ["192.0.2.20"] }]);
    expect(dig(`www.${name}`, "A", srv.dnsPort, "+dnssec").answers).toEqual(["A 192.0.2.20"]);
    expect((await req("GET", `/api/zones/${name}/dnssec`, { token: u.token })).body).toEqual({ enabled: false, nsec_mode: "nsec" });

    const on = await req("PUT", `/api/zones/${name}/dnssec`, { token: u.token, json: { enabled: true } });
    expect(on.status, on.text).toBe(200);
    // pdns publishes one DS per configured digest type (SHA-256 and SHA-384 by default).
    expect(on.body.ds.some((d: string) => /^\d+ 13 2 [0-9a-f]{64}$/i.test(d))).toBe(true);
    expect(on.body.dnskey).toStartWith("257 3 13 ");
    const signed = dig(`www.${name}`, "A", srv.dnsPort, "+dnssec");
    expect(signed.answers.map((a) => a.split(" ")[0]).sort()).toEqual(["A", "RRSIG"]);
    expect(dig(name, "DNSKEY", srv.dnsPort).answers[0]).toStartWith("DNSKEY 257 3 13 ");
    expect(dig(`nope.${name}`, "A", srv.dnsPort, "+dnssec").authority).toContain("NSEC");

    const key = await req("POST", `/api/zones/${name}/tsig-keys`, { token: u.token, json: { name: `xfr.${name}` } });
    expect(key.status).toBe(201);
    const zoneText = axfr(name, "+dnssec", "-y", `hmac-sha256:${key.body.name}:${key.body.secret}`);
    const dir = mkdtempSync(join(tmpdir(), "certo-e2e-"));
    try {
      const file = join(dir, "zone");
      // dig prints the TSIG of the signed response, which is not part of the zone.
      writeFileSync(file, zoneText.split("\n").filter((l) => !/\sTSIG\s/.test(l)).join("\n"));
      const verify = Bun.spawnSync(["ldns-verify-zone", file]);
      expect(verify.exitCode, verify.stderr.toString() + zoneText).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }

    const nsec3 = await req("PUT", `/api/zones/${name}/dnssec`, { token: u.token, json: { enabled: true, nsec_mode: "nsec3" } });
    expect(nsec3.body.nsec_mode).toBe("nsec3");
    expect(dig(`nope.${name}`, "A", srv.dnsPort, "+dnssec").authority).toContain("NSEC3");

    const off = await req("PUT", `/api/zones/${name}/dnssec`, { token: u.token, json: { enabled: false } });
    expect(off.body).toEqual({ enabled: false, nsec_mode: "nsec" });
    expect(dig(`www.${name}`, "A", srv.dnsPort, "+dnssec").answers).toEqual(["A 192.0.2.20"]);
  });

  test("AXFR is refused by default and allowed by IP or TSIG", async () => {
    const u = await newUser();
    const name = `${uniq("xfr")}.test`;
    await activeZone(u.token, name);
    await patch(u.token, name, [{ name: "www", type: "A", records: ["192.0.2.30"] }]);
    const records = (text: string) => text.split("\n").filter((l) => l && !l.startsWith(";"));
    const types = (text: string) => [...new Set(records(text).map((l) => l.split(/\s+/)[3]))].sort();

    expect(axfr(name)).toContain("Transfer failed");

    const key = await req("POST", `/api/zones/${name}/tsig-keys`, {
      token: u.token, json: { name: "secondary", algorithm: "hmac-sha512" },
    });
    expect(key.status).toBe(201);
    const signed = axfr(name, "-y", `hmac-sha512:secondary:${key.body.secret}`);
    expect(types(signed)).toEqual(["A", "NS", "SOA", "TSIG"]);
    const wrong = axfr(name, "-y", `hmac-sha512:secondary:${btoa("x".repeat(64))}`);
    expect(wrong).not.toContain("192.0.2.30");

    const allow = await req("PUT", `/api/zones/${name}/axfr`, { token: u.token, json: { allow_from: ["127.0.0.1"] } });
    expect(allow.body.allow_from).toEqual(["127.0.0.1/32"]);
    expect(allow.body.tsig_keys.map((k: any) => k.name)).toEqual(["secondary"]);
    expect(types(axfr(name))).toEqual(["A", "NS", "SOA"]);

    const removed = await req("DELETE", `/api/zones/${name}/tsig-keys/secondary`, { token: u.token });
    expect(removed.status).toBe(204);
    expect(axfr(name, "-y", `hmac-sha512:secondary:${key.body.secret}`)).not.toContain("192.0.2.30");
  });
});

/** AXFR over TCP; returns dig's raw output. */
function axfr(zone: string, ...flags: string[]): string {
  const r = Bun.spawnSync(["dig", "@127.0.0.1", "-p", String(srv.dnsPort), zone, "AXFR", ...flags]);
  return r.stdout.toString();
}
