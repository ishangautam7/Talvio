import { existsSync } from "node:fs";
import { basename, dirname, join } from "node:path";

export const DEFAULT_DATABASE_PATH = "./data/talvio.db";

/** The database file before the product was renamed from Zynveliq to Talvio. */
const LEGACY_NAME = "zynveliq.db";

/**
 * The SQLite file to open. `talvio.db` is the name now, but a deployment from
 * before the rename has its accounts in `zynveliq.db` beside it, and opening a
 * fresh `talvio.db` there would sign everyone out and lose every account. So
 * when the chosen file is a `talvio.db` that does not exist yet and a
 * `zynveliq.db` does, the old file is used (and `legacy` says so, for the log).
 * Any other explicit `DATABASE_PATH` is used exactly as given.
 */
export function resolveDatabasePath(
  configured: string | undefined,
  exists: (path: string) => boolean = existsSync,
): { path: string; legacy: boolean } {
  const path = configured ?? DEFAULT_DATABASE_PATH;
  if (basename(path) !== "talvio.db" || exists(path)) {
    return { path, legacy: false };
  }
  const old = join(dirname(path), LEGACY_NAME);
  return exists(old) ? { path: old, legacy: true } : { path, legacy: false };
}
