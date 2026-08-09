import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, newUser } from "./context";

beforeAll(async () => { await ensureServer(); });

describe("api keys", () => {
  test("default key is global", async () => {
    const u = await newUser();
    const r = await req("GET", "/api/keys", { token: u.token });
    expect(r.status).toBe(200);
    expect(r.body.length).toBe(1);
    expect(r.body[0].scope).toEqual(["*"]);
  });

  test("create scoped key, reject missing name, delete", async () => {
    const u = await newUser();
    const scoped = await req("POST", "/api/keys", { token: u.token, json: { name: "ci", scope: ["*.ci.test"] } });
    expect(scoped.status).toBe(201);
    expect(scoped.body.scope).toEqual(["*.ci.test"]);
    expect(scoped.body.key).toBeTruthy();

    expect((await req("POST", "/api/keys", { token: u.token, json: { scope: ["*"] } })).status).toBe(400);
    expect((await req("DELETE", `/api/keys/${scoped.body.id}`, { token: u.token })).status).toBe(204);
  });

  test("scoped key cannot manage keys (global_key_required)", async () => {
    const u = await newUser();
    const scoped = await req("POST", "/api/keys", { token: u.token, json: { name: "s", scope: ["*.x.test"] } });
    expect((await req("GET", "/api/keys", { token: scoped.body.key })).status).toBe(403);
  });
});
