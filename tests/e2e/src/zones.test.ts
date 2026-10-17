import { describe, test, expect, beforeAll } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ensureServer, req, newUser, dig, uniq, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

async function newZone(token: string, name: string) {
  const r = await req("POST", "/api/zones", { token, json: { name } });
  expect(r.status).toBe(201);
  return r.body;
}

async function addRecord(token: string, zone: string, rec: Record<string, unknown>) {
  const r = await req("POST", `/api/zones/${zone}/records`, { token, json: rec });
  expect(r.status).toBe(201);
  return r.body;
}

describe("hosted zones", () => {
  test("reverse zone serves PTR (dig -x), NXDOMAIN with SOA, apex SOA/NS", async () => {
    const u = await newUser();
    const zone = await newZone(u.token, "23.251.162.in-addr.arpa.");
    expect(zone.name).toBe("23.251.162.in-addr.arpa");
    await addRecord(u.token, zone.name, { name: "5", type: "PTR", data: "host.example.com" });

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

  test("forward zone: records, NODATA, CNAME, update and delete", async () => {
    const u = await newUser();
    const name = `${uniq("fwd")}.test`;
    await newZone(u.token, name);
    const a = await addRecord(u.token, name, { name: "www", type: "A", ttl: 120, data: "192.0.2.10" });
    expect(a).toMatchObject({ name: `www.${name}`, type: "A", ttl: 120, data: "192.0.2.10" });
    await addRecord(u.token, name, { name: "alias", type: "CNAME", data: `www.${name}` });
    await addRecord(u.token, name, { name: "@", type: "MX", data: `10 mail.${name}` });
    await addRecord(u.token, name, { name: "@", type: "TXT", data: '"v=spf1 -all"' });

    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual(["A 192.0.2.10"]);
    expect(dig(`alias.${name}`, "A", srv.dnsPort).answers).toEqual([`CNAME www.${name}.`, "A 192.0.2.10"]);
    expect(dig(name, "MX", srv.dnsPort).answers).toEqual([`MX 10 mail.${name}.`]);
    expect(dig(name, "TXT", srv.dnsPort).answers).toEqual(['TXT "v=spf1 -all"']);

    const nodata = dig(`www.${name}`, "AAAA", srv.dnsPort);
    expect(nodata.status).toBe("NOERROR");
    expect(nodata.answers).toEqual([]);
    expect(nodata.authority).toEqual(["SOA"]);

    const conflict = await req("POST", `/api/zones/${name}/records`, {
      token: u.token, json: { name: "www", type: "CNAME", data: "elsewhere.test" },
    });
    expect(conflict.status).toBe(400);
    expect(conflict.body.error).toBe("cname_conflict");
    const ptr = await req("POST", `/api/zones/${name}/records`, {
      token: u.token, json: { name: "x", type: "PTR", data: "h.test" },
    });
    expect(ptr.body.error).toBe("ptr_outside_reverse_zone");

    const upd = await req("PUT", `/api/zones/${name}/records/${a.id}`, {
      token: u.token, json: { name: "www", type: "A", data: "192.0.2.11" },
    });
    expect(upd.status).toBe(200);
    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual(["A 192.0.2.11"]);

    expect((await req("DELETE", `/api/zones/${name}/records/${a.id}`, { token: u.token })).status).toBe(204);
    expect(dig(`www.${name}`, "A", srv.dnsPort).answers).toEqual([]);

    expect((await req("DELETE", `/api/zones/${name}`, { token: u.token })).status).toBe(204);
    expect(dig(name, "SOA", srv.dnsPort).authoritative).toBe(false);
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
    const child = await req("POST", "/api/zones", { token: other.token, json: { name: `sub.${name}` } });
    expect(child.status).toBe(403);
    expect(child.body.error).toBe("zone_not_allowed");
    const own = await req("POST", "/api/zones", { token: owner.token, json: { name: `x.${srv.baseDomain}` } });
    expect(own.body.error).toBe("zone_not_allowed");

    const key = await req("POST", "/api/keys", { token: owner.token, json: { name: "z", scope: ["*.elsewhere.test"] } });
    const scoped = await req("GET", `/api/zones/${name}`, { token: key.body.key });
    expect(scoped.status).toBe(403);
    expect(scoped.body.error).toBe("domain_not_in_scope");
  });

  test("admin creates and lists zones for a user", async () => {
    const u = await newUser();
    const hdr = { "X-Admin-Key": srv.adminKey };
    const users = await req("GET", "/admin/users", { headers: hdr });
    const uid = users.body.find((x: any) => x.username === u.username).id;
    const name = `${uniq("adm")}.test`;
    const created = await req("POST", "/admin/zones", { headers: hdr, json: { user_id: uid, name } });
    expect(created.status).toBe(201);
    expect(created.body.owner).toBe(u.username);
    const mine = await req("GET", "/api/zones", { token: u.token });
    expect(mine.body.map((z: any) => z.name)).toContain(name);
    expect(dig(name, "SOA", srv.dnsPort).authoritative).toBe(true);
  });

  test("DNSSEC signs answers and the transferred zone verifies", async () => {
    const u = await newUser();
    const name = `${uniq("sec")}.test`;
    await newZone(u.token, name);
    await addRecord(u.token, name, { name: "www", type: "A", data: "192.0.2.20" });
    expect(dig(`www.${name}`, "A", srv.dnsPort, "+dnssec").answers).toEqual(["A 192.0.2.20"]);

    const on = await req("PUT", `/api/zones/${name}/dnssec`, { token: u.token, json: { enabled: true } });
    expect(on.status).toBe(200);
    expect(on.body.ds).toMatch(/^\d+ 13 2 [0-9a-f]{64}$/i);
    const signed = dig(`www.${name}`, "A", srv.dnsPort, "+dnssec");
    expect(signed.answers.map((a) => a.split(" ")[0])).toEqual(["A", "RRSIG"]);
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
      expect(verify.exitCode, verify.stderr.toString()).toBe(0);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }

    const off = await req("PUT", `/api/zones/${name}/dnssec`, { token: u.token, json: { enabled: false } });
    expect(off.body).toEqual({ enabled: false, nsec_mode: "nsec" });
    expect(dig(`www.${name}`, "A", srv.dnsPort, "+dnssec").answers).toEqual(["A 192.0.2.20"]);
  });

  test("AXFR is refused by default and allowed by IP or TSIG", async () => {
    const u = await newUser();
    const name = `${uniq("xfr")}.test`;
    await newZone(u.token, name);
    await addRecord(u.token, name, { name: "www", type: "A", data: "192.0.2.30" });
    const records = (text: string) => text.split("\n").filter((l) => l && !l.startsWith(";"));

    expect(axfr(name)).toContain("Transfer failed");

    const key = await req("POST", `/api/zones/${name}/tsig-keys`, {
      token: u.token, json: { name: "secondary", algorithm: "hmac-sha512" },
    });
    expect(key.status).toBe(201);
    const signed = axfr(name, "-y", `hmac-sha512:secondary:${key.body.secret}`);
    expect(records(signed).map((l) => l.split(/\s+/)[3])).toEqual(["SOA", "NS", "A", "SOA", "TSIG"]);
    const wrong = axfr(name, "-y", `hmac-sha512:secondary:${btoa("x".repeat(64))}`);
    expect(wrong).not.toContain("192.0.2.30");

    const allow = await req("PUT", `/api/zones/${name}/axfr`, { token: u.token, json: { allow_from: ["127.0.0.1"] } });
    expect(allow.body.allow_from).toEqual(["127.0.0.1/32"]);
    expect(allow.body.tsig_keys.map((k: any) => k.name)).toEqual(["secondary"]);
    expect(records(axfr(name))).toHaveLength(4);
  });
});

/** AXFR over TCP; returns dig's raw output. */
function axfr(zone: string, ...flags: string[]): string {
  const r = Bun.spawnSync(["dig", "@127.0.0.1", "-p", String(srv.dnsPort), zone, "AXFR", ...flags]);
  return r.stdout.toString();
}
