// e2e helpers: build + run the real certo binary, an HTTP client, and DNS lookups via dig.
import { mkdtempSync, writeFileSync, rmSync, mkdirSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createServer } from "node:net";

// src → tests/e2e → tests → repo root
const REPO_ROOT = resolve(import.meta.dir, "..", "..", "..");

/** Find a free TCP port (also used for the DNS UDP/TCP listener — the spaces rarely clash).
 *  Using ephemeral ports keeps the suite immune to a leftover server from a prior run. */
function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const addr = s.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      s.close(() => (port ? resolve(port) : reject(new Error("no free port"))));
    });
  });
}

/** Build the certo binary (placeholder SPA embed; API/DNS only). Honors $CERTO_BIN. */
export function buildServer(): string {
  if (process.env.CERTO_BIN) return process.env.CERTO_BIN;
  const distIndex = join(REPO_ROOT, "web", "dist", "index.html");
  if (!existsSync(distIndex)) {
    mkdirSync(join(REPO_ROOT, "web", "dist"), { recursive: true });
    writeFileSync(distIndex, "<!doctype html><title>certo e2e</title>");
  }
  const binDir = join(import.meta.dir, "..", ".bin");
  mkdirSync(binDir, { recursive: true });
  const bin = join(binDir, "certo");
  const r = Bun.spawnSync(["go", "build", "-o", bin, "."], {
    cwd: REPO_ROOT,
    env: { ...process.env, CGO_ENABLED: "0" },
  });
  if (r.exitCode !== 0) {
    throw new Error(`go build failed (exit ${r.exitCode}):\n${r.stdout?.toString() ?? ""}${r.stderr.toString()}`);
  }
  return bin;
}

export interface Server {
  apiBase: string;
  dnsPort: number;
  baseDomain: string;
  adminKey: string;
  stop(): void;
}

export interface StartOpts {
  apiPort?: number;
  dnsPort?: number;
  baseDomain?: string;
  adminKey?: string;
}

export async function startServer(opts: StartOpts = {}): Promise<Server> {
  const apiPort = opts.apiPort ?? (await freePort());
  const dnsPort = opts.dnsPort ?? (await freePort());
  const baseDomain = opts.baseDomain ?? "s.example.test";
  const adminKey = opts.adminKey ?? "e2e-admin-key";

  const bin = buildServer();
  const dir = mkdtempSync(join(tmpdir(), "certo-e2e-"));
  const config = [
    "[general]",
    `listen = "127.0.0.1:${dnsPort}"`,
    `protocol = "both"`,
    `domain = "${baseDomain}"`,
    `nsname = "${baseDomain}"`,
    `nsadmin = "admin.${baseDomain}"`,
    `records = ["${baseDomain}. A 127.0.0.1", "${baseDomain}. NS ${baseDomain}."]`,
    "",
    "[database]",
    `engine = "sqlite"`,
    `connection = "${join(dir, "certo.db")}"`,
    "",
    "[api]",
    `ip = "127.0.0.1"`,
    `port = "${apiPort}"`,
    `tls = "none"`,
    `jwt_secret = "e2e-secret"`,
    `admin_key = "${adminKey}"`,
    "",
    "[logconfig]",
    `loglevel = "warn"`,
    `logtype = "stdout"`,
    `logformat = "text"`,
    "",
  ].join("\n");
  const cfgPath = join(dir, "config.toml");
  writeFileSync(cfgPath, config);

  const proc = Bun.spawn([bin, "-c", cfgPath], { cwd: dir, stdout: "pipe", stderr: "pipe" });
  const apiBase = `http://127.0.0.1:${apiPort}`;

  const deadline = Date.now() + 20_000;
  let ready = false;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`${apiBase}/health`);
      if (r.ok) { ready = true; break; }
    } catch {
      // not up yet
    }
    await Bun.sleep(150);
  }
  if (!ready) {
    proc.kill();
    const err = await new Response(proc.stderr).text().catch(() => "");
    throw new Error(`certo did not become ready on ${apiBase}\n${err}`);
  }

  return {
    apiBase,
    dnsPort,
    baseDomain,
    adminKey,
    stop() {
      try { proc.kill(); } catch { /* already gone */ }
      try { rmSync(dir, { recursive: true, force: true }); } catch { /* best effort */ }
    },
  };
}

export interface Resp<T = any> {
  status: number;
  body: T;
  text: string;
}

export interface ReqOpts {
  token?: string;
  basic?: { user: string; pass: string };
  headers?: Record<string, string>;
  json?: unknown;
}

/** Returns a typed request helper bound to a base URL. */
export function makeClient(base: string) {
  return async function req<T = any>(method: string, path: string, opts: ReqOpts = {}): Promise<Resp<T>> {
    const headers: Record<string, string> = { ...opts.headers };
    if (opts.token) headers["Authorization"] = `Bearer ${opts.token}`;
    if (opts.basic) {
      headers["Authorization"] = "Basic " + Buffer.from(`${opts.basic.user}:${opts.basic.pass}`).toString("base64");
    }
    let body: string | undefined;
    if (opts.json !== undefined) {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(opts.json);
    }
    const r = await fetch(`${base}${path}`, { method, headers, body });
    const text = await r.text();
    let parsed: any;
    try { parsed = text ? JSON.parse(text) : undefined; } catch { parsed = undefined; }
    return { status: r.status, body: parsed as T, text };
  };
}

/** Query the certo DNS server directly for TXT values (unquoted). */
export function digTxt(name: string, port: number): string[] {
  const r = Bun.spawnSync(["dig", "@127.0.0.1", "-p", String(port), name, "TXT", "+short"]);
  const out = r.stdout.toString().trim();
  if (!out) return [];
  return out.split("\n").map((l) => l.trim().replace(/^"|"$/g, "")).filter(Boolean);
}

/** Query the certo DNS server directly for A records. */
export function digA(name: string, port: number): string[] {
  const r = Bun.spawnSync(["dig", "@127.0.0.1", "-p", String(port), name, "A", "+short"]);
  return r.stdout.toString().trim().split("\n").map((l) => l.trim()).filter(Boolean);
}

let counter = 0;
/** Unique-per-run identifier so tests don't collide in the shared DB. */
export function uniq(prefix: string): string {
  counter += 1;
  return `${prefix}${counter}`;
}

/** acme-dns requires the txt value to be exactly 43 characters. */
export const token43 = (c: string): string => c.repeat(43);
