import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, req, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("public", () => {
  test("GET /health → 200", async () => {
    expect((await req("GET", "/health")).status).toBe(200);
  });

  test("GET /api/info advertises ns-certo + capabilities", async () => {
    const r = await req("GET", "/api/info");
    expect(r.status).toBe(200);
    expect(r.body.provider).toBe("ns-certo");
    expect(r.body.base_domain).toBe(srv.baseDomain);
    expect(r.body.capabilities).toContain("acmedns");
    expect(r.body.capabilities).toContain("domain_management");
  });

  test("GET /llms.txt → 200 with content", async () => {
    const r = await req("GET", "/llms.txt");
    expect(r.status).toBe(200);
    expect(r.text.length).toBeGreaterThan(100);
  });
});
