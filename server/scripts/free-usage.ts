/**
 * Free credit usage from the server's database: the totals, the heaviest
 * accounts, and spend per kind over the last 30 days.
 *
 *   bun scripts/free-usage.ts [path/to/talvio.db]     (default: DATABASE_PATH or ./data/talvio.db)
 *   docker exec talvio-server bun scripts/free-usage.ts /data/talvio.db
 */
import { Database } from "bun:sqlite";

const path = process.argv[2] ?? process.env.DATABASE_PATH ?? "./data/talvio.db";
const db = new Database(path, { readonly: true });

const exists = db
  .query("SELECT 1 FROM sqlite_master WHERE name = 'free_totals'")
  .get();
if (!exists) {
  console.log(`${path}: no free credit has been used yet.`);
  process.exit(0);
}

const total = db
  .query(
    "SELECT COUNT(*) AS accounts, COALESCE(SUM(used_usd), 0) AS usd, COALESCE(SUM(requests), 0) AS requests FROM free_totals",
  )
  .get() as { accounts: number; usd: number; requests: number };
console.log(
  `${total.accounts} accounts, ${total.requests} requests, $${total.usd.toFixed(4)} spent in all`,
);

console.log("\nLast 30 days by kind:");
const since = Date.now() - 30 * 24 * 60 * 60 * 1000;
for (const row of db
  .query(
    `SELECT kind, model, COUNT(*) AS requests, SUM(units) AS units, unit, SUM(cost_usd) AS usd
     FROM free_usage WHERE created_at >= ? GROUP BY kind, model, unit ORDER BY usd DESC`,
  )
  .all(since) as {
  kind: string;
  model: string;
  requests: number;
  units: number;
  unit: string;
  usd: number;
}[]) {
  console.log(
    `  ${row.kind.padEnd(10)} ${row.model.padEnd(26)} ${String(row.requests).padStart(6)} req  ${Math.round(row.units)} ${row.unit}  $${row.usd.toFixed(4)}`,
  );
}

console.log("\nTop 20 accounts:");
for (const row of db
  .query(
    `SELECT t.used_usd AS usd, t.requests, t.updated_at, u.email
     FROM free_totals t LEFT JOIN users u ON u.id = t.user_id
     ORDER BY t.used_usd DESC LIMIT 20`,
  )
  .all() as {
  usd: number;
  requests: number;
  updated_at: number;
  email: string | null;
}[]) {
  console.log(
    `  $${row.usd.toFixed(4).padStart(8)} ${String(row.requests).padStart(6)} req  last ${new Date(row.updated_at).toISOString().slice(0, 16)}  ${row.email ?? "(account deleted)"}`,
  );
}
