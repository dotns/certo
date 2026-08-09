// Shared test context: a single certo server reused across all feature files.
// `bun test` runs the suite in one process, so the memoized promise yields one server;
// it is torn down on process exit.
import { startServer, makeClient, digTxt, digA, uniq, token43, type Server, type Resp, type ReqOpts } from "./helpers";

let serverPromise: Promise<Server> | null = null;

/** Start (once) and return the shared certo server. Call from each file's beforeAll. */
export function ensureServer(): Promise<Server> {
  if (!serverPromise) {
    serverPromise = startServer().then((s) => {
      process.on("exit", () => s.stop());
      return s;
    });
  }
  return serverPromise;
}

/** HTTP request against the shared server. */
export async function req<T = any>(method: string, path: string, opts: ReqOpts = {}): Promise<Resp<T>> {
  const s = await ensureServer();
  return makeClient(s.apiBase)<T>(method, path, opts);
}

/** Register a fresh user; returns its JWT and default (global) API key. */
export async function newUser() {
  const username = uniq("user");
  const reg = await req("POST", "/api/register", { json: { username, password: "password123" } });
  if (reg.status !== 201) throw new Error(`register failed: ${reg.status} ${reg.text}`);
  const keys = await req("GET", "/api/keys", { token: reg.body.token });
  return { username, token: reg.body.token as string, apiKey: keys.body[0].key as string };
}

export { digTxt, digA, uniq, token43 };
export type { Server };
