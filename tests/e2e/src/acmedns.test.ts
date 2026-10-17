import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, newUser, digTxt, uniq, token43, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("acme-dns storage + update (with DNS)", () => {
  test("storage fetch auto-provisions the account", async () => {
    const u = await newUser();
    const domain = `${uniq("ad")}.test`;
    const r = await req("GET", `/acmedns/${domain}`, { basic: { user: u.username, pass: u.apiKey } });
    expect(r.status).toBe(200);
    expect(r.body.username).toBe(u.username);
    expect(r.body.password).toBe(u.apiKey);
    expect(r.body.subdomain).toBeTruthy();
    expect(r.body.fulldomain).toBe(`${r.body.subdomain}.${srv.baseDomain}`);
    expect(r.body.server_url).toBe(srv.apiBase);
    expect((await req("GET", "/api/domains", { token: u.token })).body.map((d: any) => d.domain)).toContain(domain);
  });

  test("storage fetch with bad credentials → 401", async () => {
    const u = await newUser();
    expect((await req("GET", "/acmedns/x.test", { basic: { user: u.username, pass: "wrong" } })).status).toBe(401);
  });

  test("storage FetchAll is scope-filtered", async () => {
    const u = await newUser();
    await req("GET", "/acmedns/keep.allow.test", { basic: { user: u.username, pass: u.apiKey } });
    await req("GET", "/acmedns/skip.deny.test", { basic: { user: u.username, pass: u.apiKey } });
    const scoped = await req("POST", "/api/keys", { token: u.token, json: { name: "fa", scope: ["*.allow.test"] } });
    const all = await req("GET", "/acmedns", { basic: { user: u.username, pass: scoped.body.key } });
    expect(all.status).toBe(200);
    expect(Object.keys(all.body)).toContain("keep.allow.test");
    expect(Object.keys(all.body)).not.toContain("skip.deny.test");
  });

  test("update writes TXT and DNS resolves it", async () => {
    const u = await newUser();
    const acct = await req("GET", `/acmedns/${uniq("upd")}.test`, { basic: { user: u.username, pass: u.apiKey } });
    const sub = acct.body.subdomain;
    const txt = token43("a");
    const r = await req("POST", "/update", {
      headers: { "X-Api-User": u.username, "X-Api-Key": u.apiKey },
      json: { subdomain: sub, txt },
    });
    expect(r.status).toBe(200);
    expect(r.body.txt).toBe(txt);
    expect(digTxt(`${sub}.${srv.baseDomain}`, srv.dnsPort)).toContain(txt);
  });

  test("update validation: bad txt length / unknown subdomain / bad key", async () => {
    const u = await newUser();
    const sub = (await req("GET", `/acmedns/${uniq("v")}.test`, { basic: { user: u.username, pass: u.apiKey } })).body.subdomain;
    const hdr = { "X-Api-User": u.username, "X-Api-Key": u.apiKey };
    expect((await req("POST", "/update", { headers: hdr, json: { subdomain: sub, txt: "short" } })).status).toBe(400);
    expect((await req("POST", "/update", { headers: hdr, json: { subdomain: "deadbeef", txt: token43("a") } })).status).toBe(403);
    expect((await req("POST", "/update", { headers: { "X-Api-User": u.username, "X-Api-Key": "wrong" }, json: { subdomain: sub, txt: token43("a") } })).status).toBe(401);
  });

  test("rolling cap keeps the 2 newest values", async () => {
    const u = await newUser();
    const sub = (await req("GET", `/acmedns/${uniq("roll")}.test`, { basic: { user: u.username, pass: u.apiKey } })).body.subdomain;
    for (const c of ["1", "2", "3"]) {
      await req("POST", "/update", { headers: { "X-Api-User": u.username, "X-Api-Key": u.apiKey }, json: { subdomain: sub, txt: token43(c) } });
    }
    const txts = digTxt(`${sub}.${srv.baseDomain}`, srv.dnsPort);
    expect(txts.length).toBe(2);
    expect(txts).not.toContain(token43("1"));
    expect(txts).toContain(token43("2"));
    expect(txts).toContain(token43("3"));
  });

  test("cross-protocol: httpreq present + acme-dns update share one record", async () => {
    const u = await newUser();
    const domain = `${uniq("cross")}.test`;
    const sub = (await req("GET", `/acmedns/${domain}`, { basic: { user: u.username, pass: u.apiKey } })).body.subdomain;
    await req("POST", "/present", { basic: { user: u.username, pass: u.apiKey }, json: { fqdn: `_acme-challenge.${domain}.`, value: "via-httpreq" } });
    await req("POST", "/update", { headers: { "X-Api-User": u.username, "X-Api-Key": u.apiKey }, json: { subdomain: sub, txt: token43("c") } });
    const txts = digTxt(`${sub}.${srv.baseDomain}`, srv.dnsPort);
    expect(txts).toContain("via-httpreq");
    expect(txts).toContain(token43("c"));
  });

  test("wildcard storage key (%2A.) normalizes to the base domain", async () => {
    const u = await newUser();
    const domain = `${uniq("wild")}.test`;
    const exact = await req("GET", `/acmedns/${domain}`, { basic: { user: u.username, pass: u.apiKey } });
    const wild = await req("GET", `/acmedns/%2A.${domain}`, { basic: { user: u.username, pass: u.apiKey } });
    expect(wild.status).toBe(200);
    expect(wild.body.subdomain).toBe(exact.body.subdomain);
  });
});
