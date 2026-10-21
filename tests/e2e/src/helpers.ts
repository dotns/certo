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
  const r = Bun.spawnSync(["cargo", "build", "--locked", "-p", "certo"], { cwd: REPO_ROOT });
  if (r.exitCode !== 0) {
    throw new Error(`cargo build failed (exit ${r.exitCode}):\n${r.stdout?.toString() ?? ""}${r.stderr.toString()}`);
  }
  const bin = join(process.env.CARGO_TARGET_DIR ?? join(REPO_ROOT, "target"), "debug", "certo");
  return bin;
}

export interface Server {
  apiBase: string;
  dnsPort: number;
  baseDomain: string;
  /** The zone certo serves; the challenge domain is a name inside it. */
  zone: string;
  adminKey: string;
  /** Data directory holding `certo.db` and `pdns/`. */
  dir: string;
  /** Stops certo (and with it pdns) and starts it again on the same data and ports. */
  restart(): Promise<void>;
  stop(): void;
}

export interface StartOpts {
  apiPort?: number;
  dnsPort?: number;
  baseDomain?: string;
  zone?: string;
  adminKey?: string;
}

/** Spawns certo and waits for `/health`; the binary starts `pdns_server` from beside itself. */
async function spawnCerto(bin: string, cfgPath: string, dir: string, apiBase: string) {
  const proc = Bun.spawn([bin, "-c", cfgPath], { cwd: dir, stdout: "pipe", stderr: "pipe" });
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (proc.exitCode !== null) break;
    try {
      const r = await fetch(`${apiBase}/health`);
      if (r.ok) return proc;
    } catch {
      // not up yet
    }
    await Bun.sleep(150);
  }
  proc.kill();
  const err = await new Response(proc.stderr).text().catch(() => "");
  const out = await new Response(proc.stdout).text().catch(() => "");
  throw new Error(`certo did not become ready on ${apiBase}\n${out}${err}`);
}

export async function startServer(opts: StartOpts = {}): Promise<Server> {
  const apiPort = opts.apiPort ?? (await freePort());
  const dnsPort = opts.dnsPort ?? (await freePort());
  const pdnsApiPort = await freePort();
  const baseDomain = opts.baseDomain ?? "s.example.test";
  // Default layout: certo serves the apex and the challenge names live one label below.
  const zone = opts.zone ?? baseDomain.replace(/^[^.]+\./, "");
  const adminKey = opts.adminKey ?? "e2e-admin-key";

  const bin = buildServer();
  const dir = mkdtempSync(join(tmpdir(), "certo-e2e-"));
  const config = [
    "[general]",
    `listen = "127.0.0.1:${dnsPort}"`,
    `domain = "${zone}"`,
    `acme_domain = "${baseDomain}"`,
    `nsname = "${baseDomain}"`,
    `nsadmin = "admin.${baseDomain}"`,
    `records = ["${zone}. A 127.0.0.1", "${baseDomain}. A 127.0.0.1", "${zone}. NS ${baseDomain}."]`,
    "",
    "[database]",
    `engine = "sqlite"`,
    `connection = "${join(dir, "certo.db")}"`,
    "",
    "[pdns]",
    `dir = "${join(dir, "pdns")}"`,
    `api_port = ${pdnsApiPort}`,
    "",
    "[zones]",
    // Delegation checks go to our own pdns, which refuses unknown names: every check is
    // inconclusive, so zones stay pending until an admin exempts them.
    `resolver = "127.0.0.1:${dnsPort}"`,
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
  const apiBase = `http://127.0.0.1:${apiPort}`;
  let proc = await spawnCerto(bin, cfgPath, dir, apiBase);

  return {
    apiBase,
    dnsPort,
    baseDomain,
    zone,
    adminKey,
    dir,
    async restart() {
      proc.kill();
      await proc.exited;
      proc = await spawnCerto(bin, cfgPath, dir, apiBase);
    },
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

export interface DigResult {
  status: string;
  authoritative: boolean;
  /** `TYPE data` for each answer record, e.g. `PTR host.example.com.` */
  answers: string[];
  /** Record types in the authority section. */
  authority: string[];
}

/** Full dig query against the certo DNS server. */
export function dig(name: string, type: string, port: number, ...flags: string[]): DigResult {
  const r = Bun.spawnSync([
    "dig", "@127.0.0.1", "-p", String(port), name, type, "+norecurse",
    "+noall", "+comments", "+answer", "+authority", ...flags,
  ].filter(Boolean));
  const res: DigResult = { status: "", authoritative: false, answers: [], authority: [] };
  let section = "";
  for (const line of r.stdout.toString().split("\n")) {
    const status = line.match(/status: ([A-Z]+)/);
    if (status) res.status = status[1];
    const hdrFlags = line.match(/^;; flags: ([a-z ]+);/);
    if (hdrFlags) res.authoritative = hdrFlags[1].split(" ").includes("aa");
    const sec = line.match(/^;; ([A-Z]+) SECTION:/);
    if (sec) { section = sec[1]; continue; }
    if (!line.trim() || line.startsWith(";")) continue;
    const [, , , rtype, ...data] = line.trim().split(/\s+/);
    if (section === "ANSWER") res.answers.push(`${rtype} ${data.join(" ")}`);
    if (section === "AUTHORITY") res.authority.push(rtype);
  }
  return res;
}

let counter = 0;
/** Unique-per-run identifier so tests don't collide in the shared DB. */
export function uniq(prefix: string): string {
  counter += 1;
  return `${prefix}${counter}`;
}

/** acme-dns requires the txt value to be exactly 43 characters. */
export const token43 = (c: string): string => c.repeat(43);
