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
});
