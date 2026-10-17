import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, digTxt, token43, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("acme-dns /register (native, anonymous)", () => {
  test("register allocates an acme-<nanoid> account + 10-char subdomain, usable for /update", async () => {
    const reg = await req("POST", "/register", { json: { allowfrom: [] } });
    expect(reg.status).toBe(201);
    const { username, password, subdomain, fulldomain } = reg.body;
    expect(username).toMatch(/^acme-/);
    expect(subdomain).toHaveLength(10);
    expect(fulldomain).toBe(`${subdomain}.${srv.baseDomain}`);
    expect(password).toBeTruthy();

    // The returned creds drive the native /update (no certo account, file-storage style).
    const txt = token43("z");
    const upd = await req("POST", "/update", {
      headers: { "X-Api-User": username, "X-Api-Key": password },
      json: { subdomain, txt },
    });
    expect(upd.status).toBe(200);
    expect(digTxt(`${subdomain}.${srv.baseDomain}`, srv.dnsPort)).toContain(txt);
  });

  test("allowfrom restricts /update by source IP", async () => {
    // allowfrom that excludes loopback → update rejected
    const deny = (await req("POST", "/register", { json: { allowfrom: ["10.0.0.0/8"] } })).body;
    const denied = await req("POST", "/update", {
      headers: { "X-Api-User": deny.username, "X-Api-Key": deny.password },
      json: { subdomain: deny.subdomain, txt: token43("a") },
    });
    expect(denied.status).toBe(403);

    // allowfrom that includes loopback → update allowed
    const allow = (await req("POST", "/register", { json: { allowfrom: ["127.0.0.0/8", "::1/128"] } })).body;
    const ok = await req("POST", "/update", {
      headers: { "X-Api-User": allow.username, "X-Api-Key": allow.password },
      json: { subdomain: allow.subdomain, txt: token43("a") },
    });
    expect(ok.status).toBe(200);
  });

  test("each registration is distinct", async () => {
    const a = (await req("POST", "/register")).body.subdomain;
    const b = (await req("POST", "/register")).body.subdomain;
    expect(a).not.toBe(b);
  });

  test("POST /register hits acme-dns; GET /register still serves the SPA", async () => {
    expect((await req("POST", "/register", { json: {} })).status).toBe(201);
    // GET /register is the dashboard signup page (SPA), not the acme-dns endpoint.
    const page = await req("GET", "/register");
    expect(page.status).toBe(200);
    expect(page.text.toLowerCase()).toContain("<!doctype html");
  });
});
