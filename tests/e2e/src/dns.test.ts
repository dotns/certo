import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, digA, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("DNS server", () => {
  test("base domain A record resolves", () => {
    expect(digA(srv.baseDomain, srv.dnsPort)).toContain("127.0.0.1");
  });
});
