import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, newUser, uniq } from "./context";

beforeAll(async () => { await ensureServer(); });

describe("account & auth", () => {
  test("register, duplicate (409), short password (400)", async () => {
    const username = uniq("acct");
    const ok = await req("POST", "/api/register", { json: { username, password: "password123" } });
    expect(ok.status).toBe(201);
    expect(ok.body.token).toBeTruthy();
    expect(ok.body.username).toBe(username);

    expect((await req("POST", "/api/register", { json: { username, password: "password123" } })).status).toBe(409);
    expect((await req("POST", "/api/register", { json: { username: uniq("acct"), password: "x" } })).status).toBe(400);
  });

  test("login correct (200) / wrong (401)", async () => {
    const username = uniq("login");
    await req("POST", "/api/register", { json: { username, password: "password123" } });
    expect((await req("POST", "/api/login", { json: { username, password: "password123" } })).status).toBe(200);
    expect((await req("POST", "/api/login", { json: { username, password: "nope" } })).status).toBe(401);
  });

  test("GET /api/profile", async () => {
    const u = await newUser();
    const r = await req("GET", "/api/profile", { token: u.token });
    expect(r.status).toBe(200);
    expect(r.body.username).toBe(u.username);
  });

  test("DELETE /api/profile removes the account", async () => {
    const u = await newUser();
    expect((await req("DELETE", "/api/profile", { token: u.token })).status).toBe(204);
    expect((await req("POST", "/api/login", { json: { username: u.username, password: "password123" } })).status).toBe(401);
  });
});
