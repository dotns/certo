import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, newUser, uniq, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("domains", () => {
  test("add / list / duplicate / delete", async () => {
    const u = await newUser();
    const domain = `${uniq("d")}.test`;
    const add = await req("POST", "/api/domains", { token: u.token, json: { domain } });
    expect(add.status).toBe(201);
    expect(add.body.domain).toBe(domain);
    expect(add.body.cname_target).toBe(`${add.body.subdomain}.${srv.baseDomain}`);

    const list = await req("GET", "/api/domains", { token: u.token });
    expect(list.body.map((d: any) => d.domain)).toContain(domain);

    expect((await req("POST", "/api/domains", { token: u.token, json: { domain } })).status).toBe(409);
    expect((await req("DELETE", `/api/domains/${domain}`, { token: u.token })).status).toBe(204);
  });

  test("isolation: same domain → different subdomains per user", async () => {
    const a = await newUser();
    const b = await newUser();
    const domain = `${uniq("shared")}.test`;
    const ra = await req("POST", "/api/domains", { token: a.token, json: { domain } });
    const rb = await req("POST", "/api/domains", { token: b.token, json: { domain } });
    expect(ra.body.subdomain).not.toBe(rb.body.subdomain);
  });

  test("scoped key sees only in-scope domains", async () => {
    const u = await newUser();
    await req("POST", "/api/domains", { token: u.token, json: { domain: "in.scope.test" } });
    await req("POST", "/api/domains", { token: u.token, json: { domain: "out.other.test" } });
    const scoped = await req("POST", "/api/keys", { token: u.token, json: { name: "f", scope: ["*.scope.test"] } });
    const names = (await req("GET", "/api/domains", { token: scoped.body.key })).body.map((d: any) => d.domain);
    expect(names).toContain("in.scope.test");
    expect(names).not.toContain("out.other.test");
  });

  test("CNAME check: missing, correct and rate limited", async () => {
    const u = await newUser();
    const missing = `${uniq("nc")}.${srv.zone}`;
    const present = `${uniq("pc")}.${srv.zone}`;
    const a = await req("POST", "/api/domains", { token: u.token, json: { domain: missing } });
    const b = await req("POST", "/api/domains", { token: u.token, json: { domain: present } });
    expect(a.status).toBe(201);
    expect(b.status).toBe(201);
    // Publish the expected CNAME inside the base zone, which the e2e resolver (pdns) serves.
    const patch = await req("PATCH", `/admin/zones/${srv.zone}/rrsets`, {
      headers: { "X-Admin-Key": srv.adminKey },
      json: { rrsets: [{ name: `_acme-challenge.${present}`, type: "CNAME", records: [`${b.body.cname_target}.`] }] },
    });
    expect(patch.status).toBe(200);

    expect((await req("GET", `/api/domains/${uniq("none")}.test/check`, { token: u.token })).status).toBe(404);
    const r1 = await req("GET", `/api/domains/${missing}/check`, { token: u.token });
    expect(r1.status).toBe(200);
    expect(r1.body).toEqual({ name: `_acme-challenge.${missing}`, expected: a.body.cname_target, observed: null, ok: false });
    const r2 = await req("GET", `/api/domains/${present}/check`, { token: u.token });
    expect(r2.status).toBe(200);
    expect(r2.body.observed).toBe(b.body.cname_target);
    expect(r2.body.ok).toBe(true);
    expect((await req("GET", `/api/domains/${missing}/check`, { token: u.token })).status).toBe(429);
  });
});
