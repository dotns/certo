import { describe, test, expect, beforeAll } from "bun:test";
import { ensureServer, dig, digA, type Server } from "./context";

let srv: Server;
beforeAll(async () => { srv = await ensureServer(); });

describe("DNS server (PowerDNS)", () => {
  test("base domain A and NS records resolve authoritatively", () => {
    expect(digA(srv.baseDomain, srv.dnsPort)).toContain("127.0.0.1");
    const ns = dig(srv.baseDomain, "NS", srv.dnsPort);
    expect(ns.authoritative).toBe(true);
    expect(ns.answers).toEqual([`NS ${srv.baseDomain}.`]);
    expect(dig(srv.baseDomain, "SOA", srv.dnsPort).answers[0]).toStartWith(`SOA ${srv.baseDomain}. admin.${srv.baseDomain}.`);
  });

  test("names outside every zone are refused", () => {
    const r = dig("nobody.example", "A", srv.dnsPort);
    expect(r.status).toBe("REFUSED");
    expect(r.authoritative).toBe(false);
  });

  test("unknown names under the base domain are NXDOMAIN with SOA", () => {
    const r = dig(`missing.${srv.baseDomain}`, "TXT", srv.dnsPort);
    expect(r.status).toBe("NXDOMAIN");
    expect(r.authority).toEqual(["SOA"]);
  });
});
