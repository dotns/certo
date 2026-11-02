import { describe, test, expect, beforeAll } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ensureServer, req, token43, uniq, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("admin (X-Admin-Key)", () => {
  const adminHdr = () => ({ "X-Admin-Key": srv.adminKey });

  test("wrong admin key → 401", async () => {
    expect((await req("GET", "/admin/users", { headers: { "X-Admin-Key": "nope" } })).status).toBe(401);
  });

  test("list / create / delete user", async () => {
    expect((await req("GET", "/admin/users", { headers: adminHdr() })).status).toBe(200);

    const username = uniq("admincreated");
    const create = await req("POST", "/admin/users", { headers: adminHdr(), json: { username, password: "password123" } });
    expect(create.status).toBe(201);
    expect(create.body.username).toBe(username);

    expect((await req("DELETE", `/admin/users/${create.body.id}`, { headers: adminHdr() })).status).toBe(204);
  });

  test("reset a user's password", async () => {
    const username = uniq("reset");
    const id = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username, password: "password123" } })).body.id;
    expect((await req("PUT", `/admin/users/${id}/password`, { headers: adminHdr(), json: { password: "x" } })).status).toBe(400);
    expect((await req("PUT", `/admin/users/999999/password`, { headers: adminHdr(), json: { password: "password456" } })).status).toBe(404);
    expect((await req("PUT", `/admin/users/${id}/password`, { headers: adminHdr(), json: { password: "password456" } })).status).toBe(204);
    expect((await req("POST", "/api/login", { json: { username, password: "password456" } })).status).toBe(200);
  });

  test("add domain for a user; list domains and records", async () => {
    const uid = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username: uniq("adm"), password: "password123" } })).body.id;
    const add = await req("POST", "/admin/domains", { headers: adminHdr(), json: { user_id: uid, domain: `${uniq("admindom")}.test` } });
    expect(add.status).toBe(201);
    expect(add.body.cname_target).toBeTruthy();
    expect((await req("GET", "/admin/domains", { headers: adminHdr() })).status).toBe(200);
    expect((await req("GET", "/admin/records", { headers: adminHdr() })).status).toBe(200);
  });
});

describe("admin bulk delete and cleanup", () => {
  const adminHdr = () => ({ "X-Admin-Key": srv.adminKey });

  test("POST /admin/users/delete removes several users at once", async () => {
    const a = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username: uniq("bulk"), password: "password123" } })).body.id;
    const b = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username: uniq("bulk"), password: "password123" } })).body.id;
    expect((await req("POST", "/admin/users/delete", { headers: adminHdr(), json: { ids: [] } })).status).toBe(400);
    const del = await req("POST", "/admin/users/delete", { headers: adminHdr(), json: { ids: [a, b, 999999] } });
    expect(del.status, del.text).toBe(200);
    expect(del.body).toEqual({ deleted: 2 });
    const users = await req("GET", "/admin/users", { headers: adminHdr() });
    expect(users.body.map((u: any) => u.id)).not.toContain(a);
  });

  test("POST /admin/domains/delete removes a domain from every account", async () => {
    const a = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username: uniq("bd"), password: "password123" } })).body.id;
    const b = (await req("POST", "/admin/users", { headers: adminHdr(), json: { username: uniq("bd"), password: "password123" } })).body.id;
    const domain = `${uniq("shared")}.test`;
    for (const uid of [a, b])
      expect((await req("POST", "/admin/domains", { headers: adminHdr(), json: { user_id: uid, domain } })).status).toBe(201);
    const del = await req("POST", "/admin/domains/delete", { headers: adminHdr(), json: { domains: [domain, "missing.test"] } });
    expect(del.status, del.text).toBe(200);
    expect(del.body).toEqual({ deleted: 2 });
    const left = await req("GET", "/admin/domains", { headers: adminHdr() });
    expect(left.body.filter((d: any) => d.domain === domain)).toEqual([]);
  });

  test("cleanup previews and deletes unused acme-dns accounts", async () => {
    const unused = (await req("POST", "/register", { json: { allowfrom: [] } })).body;
    const used = (await req("POST", "/register", { json: { allowfrom: [] } })).body;
    const upd = await req("POST", "/update", {
      headers: { "X-Api-User": used.username, "X-Api-Key": used.password },
      json: { subdomain: used.subdomain, txt: token43("u") },
    });
    expect(upd.status, upd.text).toBe(200);
    // created_at has second resolution; "older than 0 days" means created before this second.
    await Bun.sleep(1100);
    const preview = await req("GET", "/admin/cleanup?older_than_days=0", { headers: adminHdr() });
    expect(preview.status, preview.text).toBe(200);
    expect(preview.body.older_than_days).toBe(0);
    expect(preview.body.accounts).toBeGreaterThanOrEqual(1);
    expect((await req("GET", "/admin/cleanup", { headers: adminHdr() })).body).toMatchObject({ accounts: 0, older_than_days: 7 });
    const run = await req("POST", "/admin/cleanup", { headers: adminHdr(), json: { older_than_days: 0 } });
    expect(run.status, run.text).toBe(200);
    expect(run.body.deleted).toBe(preview.body.accounts);
    const names = (await req("GET", "/admin/users", { headers: adminHdr() })).body.map((u: any) => u.username);
    expect(names).not.toContain(unused.username);
    expect(names).toContain(used.username);
  });

  test("pdns.conf carries the amplification hardening settings", () => {
    const conf = readFileSync(join(srv.dir, "pdns", "pdns.conf"), "utf8");
    expect(conf).toContain("any-to-tcp=yes\n");
    expect(conf).toContain("udp-truncation-threshold=1232\n");
  });
});

describe("admin sync", () => {
  const adminHdr = () => ({ "X-Admin-Key": srv.adminKey });

  test("GET reports no drift; POST rebuilds and reports", async () => {
    const status = await req("GET", "/admin/sync", { headers: adminHdr() });
    expect(status.status).toBe(200);
    expect(status.body.zones).toEqual([]);
    expect(status.body.db_version).toBeGreaterThan(0);
    expect(status.body.pdns_version).toBe(status.body.db_version);
    const run = await req("POST", "/admin/sync", { headers: adminHdr() });
    expect(run.status, run.text).toBe(200);
    expect(run.body).toMatchObject({ created: 0, removed: 0, records_adopted: 0, zones: [] });
    expect(run.body.pdns_version).toBe(run.body.db_version);
    expect((await req("POST", "/admin/sync", { headers: { "X-Admin-Key": "nope" } })).status).toBe(401);
  });
});
