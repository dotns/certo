import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, uniq, type Server } from "./context";

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
