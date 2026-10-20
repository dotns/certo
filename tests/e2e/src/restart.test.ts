// A dedicated server: certo is restarted and the pdns database is wiped in between.
import { describe, test, expect, beforeAll, afterAll } from "bun:test";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { startServer, makeClient, digTxt, dig, uniq, type Server } from "./helpers";

let srv: Server;
beforeAll(async () => { srv = await startServer(); });
afterAll(() => srv?.stop());

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

    // pdns data lost: the base zone and the challenge records come back from certo's DB.
    rmSync(join(srv.dir, "pdns", "pdns.sqlite3"), { force: true });
    await srv.restart();
    expect(dig(srv.baseDomain, "SOA", srv.dnsPort).authoritative).toBe(true);
    expect(digTxt(fqdn, srv.dnsPort)).toEqual(["survives-restart"]);
    const clean = await req("POST", "/cleanup", {
      basic: { user: username, pass: apiKey },
      json: { fqdn: `_acme-challenge.${domain}.`, value: "survives-restart" },
    });
    expect(clean.status).toBe(200);
    expect(digTxt(fqdn, srv.dnsPort)).toEqual([]);
  });
});
