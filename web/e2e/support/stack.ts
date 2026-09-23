import { type ChildProcess, spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, readdir, rm, utimes, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeJpeg } from './images';

export const ADMIN_PASSWORD = 'e2e-password';

export interface ManifestPhoto {
  hash: string;
  w: number;
  h: number;
  effective_date: string;
  date_source: string;
  favorite: boolean;
  tags: string[];
  rotation: number;
  media_rotation: number;
}

export interface Manifest {
  generation: number;
  photos: ManifestPhoto[];
  tags: { name: string; count: number }[];
}

/** A real photoframe-web and photoframe-indexer over a throwaway volume. */
export class Stack {
  private procs: ChildProcess[] = [];
  private log = '';

  private constructor(
    readonly url: string,
    readonly dir: string,
    readonly volume: string,
  ) {}

  static async start(photos: number): Promise<Stack> {
    const here = path.dirname(fileURLToPath(import.meta.url));
    const bins = process.env.PHOTOFRAME_BIN_DIR ?? path.resolve(here, '../../../target/debug');
    for (const b of ['photoframe-web', 'photoframe-indexer']) {
      if (!existsSync(path.join(bins, b))) {
        throw new Error(`${b} not found in ${bins}. Run: cargo build -p photoframe-web -p photoframe-indexer`);
      }
    }
    const dir = await mkdtemp(path.join(tmpdir(), 'pf-e2e-'));
    const volume = path.join(dir, 'vol');
    const port = await freePort();
    const stack = new Stack(`http://127.0.0.1:${port}`, dir, volume);

    await stack.addPhotos(photos);
    const env = {
      ...process.env,
      DATABASE_URL: `sqlite://${path.join(dir, 'photoframe.db')}`,
      LIBRARY_ROOT: volume,
      ADMIN_PASSWORD,
      BIND_ADDR: `127.0.0.1:${port}`,
      // Short intervals: the indexer also checks requests and the scan flag every 2 s.
      SCAN_INTERVAL_SECS: '2',
      INGEST_QUIET_SECS: '0',
      LOG_LEVEL: 'warn',
    };
    for (const b of ['photoframe-indexer', 'photoframe-web']) {
      const p = spawn(path.join(bins, b), [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
      p.stdout.on('data', (d) => (stack.log += `[${b}] ${d}`));
      p.stderr.on('data', (d) => (stack.log += `[${b}] ${d}`));
      p.on('exit', (code, signal) => (stack.log += `[${b}] process exited: code=${code} signal=${signal}\n`));
      stack.procs.push(p);
    }
    try {
      await stack.until(async () => (await fetch(`${stack.url}/api/status`).catch(() => undefined))?.ok, 30_000, 'the server to start');
      if (photos > 0) await stack.waitForIndexed(photos);
    } catch (e) {
      throw new Error(`${e instanceof Error ? e.message : e}\n--- process output ---\n${stack.log}`);
    }
    return stack;
  }

  /** Write `count` distinct photographs straight into `library/`, each with its own file date. */
  async addPhotos(count: number, firstSeed = 0): Promise<void> {
    for (let i = 0; i < count; i++) {
      const seed = firstSeed + i;
      const day = new Date(Date.UTC(2020, 0, 1) + seed * 9 * 86_400_000);
      const file = path.join(this.volume, 'library', String(day.getUTCFullYear()), `photo-${String(seed).padStart(3, '0')}.jpg`);
      await mkdir(path.dirname(file), { recursive: true });
      // Alternate shapes so the grid and the frame see both orientations.
      await writeFile(file, makeJpeg(seed % 2 ? 240 : 320, seed % 2 ? 320 : 240, seed));
      await utimes(file, day, day);
    }
  }

  async json<T>(p: string): Promise<T> {
    const res = await fetch(`${this.url}${p}`);
    if (!res.ok) throw new Error(`${p}: ${res.status}`);
    return (await res.json()) as T;
  }

  /** A management call with HTTP Basic, for setting up state without the UI. */
  async call(method: string, p: string, body?: unknown): Promise<Response> {
    const res = await fetch(`${this.url}${p}`, {
      method,
      headers: {
        Authorization: `Basic ${Buffer.from(`admin:${ADMIN_PASSWORD}`).toString('base64')}`,
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw new Error(`${method} ${p}: ${res.status}`);
    return res;
  }

  manifest(): Promise<Manifest> {
    return this.json<Manifest>('/api/manifest');
  }

  async photo(hash: string): Promise<ManifestPhoto | undefined> {
    return (await this.manifest()).photos.find((p) => p.hash === hash);
  }

  async waitForIndexed(count: number): Promise<void> {
    await this.until(
      async () => {
        const s = await this.json<{ photo_count: number; derivative_queue: number }>('/api/status');
        return s.photo_count === count && s.derivative_queue === 0;
      },
      60_000,
      `${count} photographs to be indexed`,
    );
  }

  /** Files for one photograph under `derivatives/`, by name. */
  async derivativeFiles(hash: string): Promise<string[]> {
    const dir = path.join(this.volume, 'derivatives', hash.slice(0, 2), hash.slice(2, 4));
    return (await readdir(dir).catch(() => [])).filter((f) => f.startsWith(`${hash}-`)).sort();
  }

  async libraryFiles(area = 'library'): Promise<string[]> {
    const out: string[] = [];
    const walk = async (d: string) => {
      for (const e of await readdir(d, { withFileTypes: true }).catch(() => [])) {
        if (e.isDirectory()) await walk(path.join(d, e.name));
        else out.push(path.join(d, e.name));
      }
    };
    await walk(path.join(this.volume, area));
    return out;
  }

  async until(check: () => Promise<unknown>, timeoutMs: number, what: string): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (await check().catch(() => false)) return;
      await new Promise((r) => setTimeout(r, 250));
    }
    throw new Error(`timed out waiting for ${what}`);
  }

  logs(): string {
    return this.log;
  }

  async stop(): Promise<void> {
    for (const p of this.procs) p.kill('SIGTERM');
    await Promise.all(this.procs.map((p) => new Promise((r) => (p.exitCode !== null ? r(null) : p.once('exit', r)))));
    await rm(this.dir, { recursive: true, force: true });
  }
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address() as net.AddressInfo;
      srv.close(() => resolve(port));
    });
  });
}
