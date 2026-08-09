import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, newUser, digTxt, uniq, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("httpreq /present + /cleanup (with DNS)", () => {
  test("present existing domain → DNS resolves; cleanup removes", async () => {
    const u = await newUser();
    const domain = `${uniq("hp")}.test`;
    const add = await req("POST", "/api/domains", { token: u.token, json: { domain } });
    const fqdn = `${add.body.subdomain}.${srv.baseDomain}`;

    const pres = await req("POST", "/present", {
      basic: { user: u.username, pass: u.apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "hp-token" },
    });
    expect(pres.status).toBe(200);
    expect(digTxt(fqdn, srv.dnsPort)).toContain("hp-token");

    const clean = await req("POST", "/cleanup", {
      basic: { user: u.username, pass: u.apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "hp-token" },
    });
    expect(clean.status).toBe(200);
    expect(digTxt(fqdn, srv.dnsPort)).not.toContain("hp-token");
  });

  test("global key auto-creates an unknown domain", async () => {
    const u = await newUser();
    const domain = `${uniq("auto")}.test`;
    const pres = await req("POST", "/present", {
      basic: { user: u.username, pass: u.apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "auto-token" },
    });
    expect(pres.status).toBe(200);
    const created = (await req("GET", "/api/domains", { token: u.token })).body.find((d: any) => d.domain === domain);
    expect(created).toBeTruthy();
    expect(digTxt(created.cname_target, srv.dnsPort)).toContain("auto-token");
  });

  test("exact-scope key creates its own domain, rejects out-of-scope", async () => {
    const u = await newUser();
    const own = `${uniq("own")}.test`;
    const exact = await req("POST", "/api/keys", { token: u.token, json: { name: "ex", scope: [own] } });
    const k = exact.body.key;

    expect((await req("POST", "/present", {
      basic: { user: u.username, pass: k },
      json: { fqdn: `_acme-challenge.${own}.`, value: "own-token" },
    })).status).toBe(200);

    expect((await req("POST", "/present", {
      basic: { user: u.username, pass: k },
      json: { fqdn: `_acme-challenge.elsewhere.test.`, value: "x" },
    })).status).toBe(403);
  });
});
