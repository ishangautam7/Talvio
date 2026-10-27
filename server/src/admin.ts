import { checkPassword } from "./account";
import { adminPage } from "./adminPage";
import type { Ctx } from "./app";
import { randomToken, sha256 } from "./crypto";
import type { DeviceRow, UserRow } from "./db";
import { subjectFor } from "./free/ledger";
import { adminStart, redeemLoginCode } from "./google";
import {
  ApiError,
  invalidRequest,
  json,
  noContent,
  notFound,
  rateLimited,
  readJson,
  unauthorized,
} from "./http";

/**
 * The owner's dashboard (PROTOCOL.md, "Admin"): who signed up, which devices
 * they use and what their free credit went on, read by an account listed in
 * `ADMIN_EMAILS` signed in to `/admin`, or with `ADMIN_TOKEN` from a script.
 *
 * Nothing here returns a session token, a token hash, a password hash, a stored
 * API key or anything a person said or typed: the free credit ledger only ever
 * held the kind of request, the model, its size and its cost.
 */

const BEARER = /^Bearer\s+(\S+)$/i;
const DAY_MS = 24 * 60 * 60 * 1000;
/** Days in the daily series, today included. */
export const SERIES_DAYS = 30;
/** A user's most recent metered requests in the detail view. */
const RECENT_REQUESTS = 50;
const MAX_QUERY_CHARS = 200;

const sameSecret = (a: string, b: string) => sha256(a) === sha256(b);

/** How long an admin sign-in lasts. */
export const ADMIN_SESSION_MS = 12 * 60 * 60 * 1000;

/**
 * Admin sign-ins from the dashboard: random tokens, kept only as hashes and
 * only in this process's memory, so a restart signs the dashboard out (the
 * server runs as one instance, see app.ts). They are not devices: an admin
 * sign-in never appears in the apps' device lists.
 */
export class AdminSessions {
  private readonly byHash = new Map<
    string,
    { userId: string; expiresAt: number }
  >();

  constructor(private readonly now: () => number) {}

  create(userId: string): { token: string; expiresAt: number } {
    const token = `zva_${randomToken()}`;
    const expiresAt = this.now() + ADMIN_SESSION_MS;
    this.byHash.set(sha256(token), { userId, expiresAt });
    return { token, expiresAt };
  }

  /** The account a live admin sign-in belongs to. */
  userId(token: string): string | null {
    const session = this.byHash.get(sha256(token));
    if (!session) return null;
    if (session.expiresAt <= this.now()) {
      this.byHash.delete(sha256(token));
      return null;
    }
    return session.userId;
  }

  delete(token: string): void {
    this.byHash.delete(sha256(token));
  }

  prune(): void {
    const now = this.now();
    for (const [hash, session] of this.byHash)
      if (session.expiresAt <= now) this.byHash.delete(hash);
  }
}

/** The admin emails from `ADMIN_EMAILS` (comma separated), trimmed and lower-cased. */
export function adminEmailsFromEnv(value: string | undefined): string[] {
  return (value ?? "")
    .split(",")
    .map((address) => address.trim().toLowerCase())
    .filter(Boolean);
}

/** Whether the server has any way in to its admin routes. */
const adminEnabled = (ctx: Ctx) =>
  ctx.adminToken !== null || ctx.adminEmails.size > 0;

const isAdmin = (ctx: Ctx, user: UserRow | null): user is UserRow =>
  user !== null && ctx.adminEmails.has(user.email.toLowerCase());

/**
 * Throws unless the request carries `ADMIN_TOKEN` or a dashboard sign-in of an
 * account still listed in `ADMIN_EMAILS`; `not_found` when neither is set, so
 * such a server does not even admit to having admin routes. After 10 wrong
 * tokens in 15 minutes an address is `rate_limited`.
 */
export function requireAdmin(req: Request, ctx: Ctx, clientIp: string): void {
  if (!adminEnabled(ctx)) throw notFound();
  const failures = ctx.feedbackLimits.adminFailures;
  const wait = failures.retryAfter(clientIp);
  if (wait > 0) throw rateLimited(wait);
  const token = BEARER.exec(req.headers.get("authorization") ?? "")?.[1];
  if (token) {
    // Compared as hashes, so the time taken says nothing about the token.
    if (ctx.adminToken && sameSecret(token, ctx.adminToken)) return;
    const userId = ctx.adminSessions.userId(token);
    // Checked on every request, so taking an address out of ADMIN_EMAILS
    // (and restarting) or deleting the account ends its access.
    if (userId && isAdmin(ctx, ctx.store.userById(userId))) return;
  }
  failures.record(clientIp);
  throw unauthorized();
}

const notAdmin = () =>
  new ApiError(
    403,
    "not_admin",
    "This account is not an admin of this server.",
  );

function adminSignedIn(ctx: Ctx, user: UserRow): Response {
  const { token, expiresAt } = ctx.adminSessions.create(user.id);
  ctx.log("admin signed in");
  return json({
    token,
    expiresAt,
    user: { id: user.id, email: user.email, name: user.name },
  });
}

// ── POST /v1/admin/login, POST /v1/admin/google/exchange, POST /v1/admin/logout

/** Signs the dashboard in with an admin account's email and password. */
export async function adminLogin(req: Request, ctx: Ctx): Promise<Response> {
  if (!adminEnabled(ctx)) throw notFound();
  const body = await readJson(req);
  const user = await checkPassword(ctx, body.email, body.password);
  if (!isAdmin(ctx, user)) throw notAdmin();
  return adminSignedIn(ctx, user);
}

/** Signs the dashboard in with the code Google's sign-in brought back to `/admin`. */
export async function adminGoogleExchange(
  req: Request,
  ctx: Ctx,
): Promise<Response> {
  if (!adminEnabled(ctx)) throw notFound();
  const user = redeemLoginCode(ctx, await readJson(req));
  if (!isAdmin(ctx, user)) throw notAdmin();
  return adminSignedIn(ctx, user);
}

export function adminGoogleStart(url: URL, ctx: Ctx): Response {
  if (!adminEnabled(ctx)) throw notFound();
  return adminStart(url, ctx);
}

/** Ends a dashboard sign-in; 204 whether or not it was still live. */
export function adminLogout(req: Request, ctx: Ctx): Response {
  if (!adminEnabled(ctx)) throw notFound();
  const token = BEARER.exec(req.headers.get("authorization") ?? "")?.[1];
  if (token) ctx.adminSessions.delete(token);
  return noContent();
}

/** The dashboard page, offered only where some way in is configured. */
export function adminPageRoute(ctx: Ctx): Response {
  if (!adminEnabled(ctx)) throw notFound();
  return adminPage();
}

/** A free credit request's kind, as the dashboard names the service. */
export const SERVICES = ["chat", "transcribe", "speech"] as const;

interface ServiceUse {
  kind: string;
  requests: number;
  cost_usd: number;
  last_at: number;
}

const round = (usd: number) => Math.round(usd * 1e8) / 1e8;

const serviceJson = (rows: ServiceUse[]) =>
  rows.map((row) => ({
    service: row.kind,
    requests: row.requests,
    costUsd: round(row.cost_usd),
    lastUsedAt: row.last_at,
  }));

/** `YYYY-MM-DD` of a UTC day. */
const dayOf = (ms: number) => new Date(ms).toISOString().slice(0, 10);

// ── GET /v1/admin/overview ─────────────────────────────────────────────────

export function overview(req: Request, ctx: Ctx, clientIp: string): Response {
  requireAdmin(req, ctx, clientIp);
  const db = ctx.store.database;
  const now = ctx.now();
  const count = (sql: string, ...params: number[]) =>
    db.query<{ n: number }, number[]>(sql).get(...params)!.n;

  const users = {
    total: count("SELECT COUNT(*) AS n FROM users"),
    new24h: count(
      "SELECT COUNT(*) AS n FROM users WHERE created_at > ?",
      now - DAY_MS,
    ),
    new7d: count(
      "SELECT COUNT(*) AS n FROM users WHERE created_at > ?",
      now - 7 * DAY_MS,
    ),
    active24h: count(
      "SELECT COUNT(DISTINCT user_id) AS n FROM devices WHERE last_seen_at > ?",
      now - DAY_MS,
    ),
    active7d: count(
      "SELECT COUNT(DISTINCT user_id) AS n FROM devices WHERE last_seen_at > ?",
      now - 7 * DAY_MS,
    ),
  };

  const kinds = db
    .query<{ kind: string; n: number }, []>(
      "SELECT kind, COUNT(*) AS n FROM devices GROUP BY kind",
    )
    .all();
  const devices = {
    total: kinds.reduce((sum, row) => sum + row.n, 0),
    desktop: kinds.find((row) => row.kind === "desktop")?.n ?? 0,
    mobile: kinds.find((row) => row.kind === "mobile")?.n ?? 0,
    online: ctx.hub.onlineCount(),
  };

  const totals = db
    .query<{ requests: number; cost: number | null; subjects: number }, []>(
      "SELECT COUNT(*) AS requests, SUM(cost_usd) AS cost, COUNT(DISTINCT subject) AS subjects FROM free_usage",
    )
    .get()!;
  const byService = db
    .query<ServiceUse, []>(
      `SELECT kind, COUNT(*) AS requests, SUM(cost_usd) AS cost_usd, MAX(created_at) AS last_at
       FROM free_usage GROUP BY kind ORDER BY cost_usd DESC`,
    )
    .all();
  const byModel = db
    .query<
      { kind: string; model: string; requests: number; cost_usd: number },
      []
    >(
      `SELECT kind, model, COUNT(*) AS requests, SUM(cost_usd) AS cost_usd
       FROM free_usage GROUP BY kind, model ORDER BY cost_usd DESC`,
    )
    .all();
  const exhausted = count(
    "SELECT COUNT(*) AS n FROM free_totals WHERE used_usd >= ?",
    ctx.free.config.limitUsd - 1e-9,
  );

  // UTC days, oldest first, today last; a day with nothing is still listed.
  const today = Math.floor(now / DAY_MS);
  const from = (today - SERIES_DAYS + 1) * DAY_MS;
  const usageDays = db
    .query<{ day: number; requests: number; cost_usd: number }, [number]>(
      `SELECT created_at / ${DAY_MS} AS day, COUNT(*) AS requests, SUM(cost_usd) AS cost_usd
       FROM free_usage WHERE created_at >= ? GROUP BY day`,
    )
    .all(from);
  const signupDays = db
    .query<{ day: number; n: number }, [number]>(
      `SELECT created_at / ${DAY_MS} AS day, COUNT(*) AS n
       FROM users WHERE created_at >= ? GROUP BY day`,
    )
    .all(from);
  const days = Array.from({ length: SERIES_DAYS }, (_, i) => {
    const day = today - SERIES_DAYS + 1 + i;
    const usage = usageDays.find((row) => row.day === day);
    return {
      day: dayOf(day * DAY_MS),
      requests: usage?.requests ?? 0,
      costUsd: round(usage?.cost_usd ?? 0),
      signups: signupDays.find((row) => row.day === day)?.n ?? 0,
    };
  });

  return json({
    generatedAt: now,
    users,
    devices,
    credit: {
      enabled: ctx.free.enabled,
      limitUsd: ctx.free.config.limitUsd,
      spentUsd: round(totals.cost ?? 0),
      requests: totals.requests,
      accountsWithUsage: totals.subjects,
      accountsExhausted: exhausted,
      byService: serviceJson(byService),
      byModel: byModel.map((row) => ({
        service: row.kind,
        model: row.model,
        requests: row.requests,
        costUsd: round(row.cost_usd),
      })),
    },
    days,
    feedback: {
      total: count("SELECT COUNT(*) AS n FROM feedback"),
      last7d: count(
        "SELECT COUNT(*) AS n FROM feedback WHERE created_at > ?",
        now - 7 * DAY_MS,
      ),
      url: `${ctx.publicUrl}/v1/admin/feedback`,
    },
  });
}

// ── GET /v1/admin/users ────────────────────────────────────────────────────

interface UserListRow {
  id: string;
  email: string;
  name: string | null;
  created_at: number;
  devices: number;
  desktops: number;
  mobiles: number;
  last_seen_at: number | null;
}

/** Credit and services per user, by ledger subject (the email's hash). */
function creditFor(ctx: Ctx, emails: string[]) {
  const db = ctx.store.database;
  const subjects = emails.map(subjectFor);
  const out = new Map<
    string,
    {
      usedUsd: number;
      requests: number;
      lastUsedAt: number | null;
      services: ServiceUse[];
    }
  >();
  if (subjects.length === 0) return out;
  const marks = subjects.map(() => "?").join(", ");
  const totals = db
    .query<
      {
        subject: string;
        used_usd: number;
        requests: number;
        updated_at: number;
      },
      string[]
    >(
      `SELECT subject, used_usd, requests, updated_at FROM free_totals WHERE subject IN (${marks})`,
    )
    .all(...subjects);
  const services = db
    .query<ServiceUse & { subject: string }, string[]>(
      `SELECT subject, kind, COUNT(*) AS requests, SUM(cost_usd) AS cost_usd, MAX(created_at) AS last_at
       FROM free_usage WHERE subject IN (${marks}) GROUP BY subject, kind ORDER BY cost_usd DESC`,
    )
    .all(...subjects);
  emails.forEach((email, i) => {
    const subject = subjects[i]!;
    const total = totals.find((row) => row.subject === subject);
    out.set(email, {
      usedUsd: round(total?.used_usd ?? 0),
      requests: total?.requests ?? 0,
      lastUsedAt: total?.updated_at ?? null,
      services: services.filter((row) => row.subject === subject),
    });
  });
  return out;
}

function creditJson(
  ctx: Ctx,
  credit: ReturnType<typeof creditFor> extends Map<string, infer V> ? V : never,
) {
  const limitUsd = ctx.free.config.limitUsd;
  return {
    limitUsd,
    usedUsd: credit.usedUsd,
    remainingUsd: round(Math.max(0, limitUsd - credit.usedUsd)),
    requests: credit.requests,
    lastUsedAt: credit.lastUsedAt,
  };
}

const SORTS: Record<string, string> = {
  new: "u.created_at DESC",
  seen: "last_seen_at IS NULL, last_seen_at DESC, u.created_at DESC",
  email: "u.email ASC",
};

export function listUsers(
  req: Request,
  ctx: Ctx,
  url: URL,
  clientIp: string,
): Response {
  requireAdmin(req, ctx, clientIp);
  const limitParam = url.searchParams.get("limit");
  const limit = limitParam === null ? 50 : Number(limitParam);
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw invalidRequest("limit must be 1 to 100.");
  }
  const offsetParam = url.searchParams.get("offset");
  const offset = offsetParam === null ? 0 : Number(offsetParam);
  if (!Number.isInteger(offset) || offset < 0) {
    throw invalidRequest("offset must be 0 or more.");
  }
  const sort = url.searchParams.get("sort") ?? "new";
  if (!(sort in SORTS))
    throw invalidRequest("sort must be new, seen or email.");
  const q = (url.searchParams.get("q") ?? "").trim().toLowerCase();
  if (q.length > MAX_QUERY_CHARS) {
    throw invalidRequest(`q must be at most ${MAX_QUERY_CHARS} characters.`);
  }
  // LIKE wildcards in the query match themselves.
  const pattern = `%${q.replace(/[\\%_]/g, (c) => `\\${c}`)}%`;
  const where = q
    ? "WHERE u.email LIKE ?1 ESCAPE '\\' OR lower(coalesce(u.name, '')) LIKE ?1 ESCAPE '\\' OR u.id = ?2"
    : "";
  const params = q ? [pattern, q] : [];

  const db = ctx.store.database;
  const total = db
    .query<{ n: number }, string[]>(
      `SELECT COUNT(*) AS n FROM users u ${where}`,
    )
    .get(...params)!.n;
  const rows = db
    .query<UserListRow, (string | number)[]>(
      `SELECT u.id, u.email, u.name, u.created_at,
              COUNT(d.id) AS devices,
              COALESCE(SUM(d.kind = 'desktop'), 0) AS desktops,
              COALESCE(SUM(d.kind = 'mobile'), 0) AS mobiles,
              MAX(d.last_seen_at) AS last_seen_at
       FROM users u LEFT JOIN devices d ON d.user_id = u.id
       ${where}
       GROUP BY u.id
       ORDER BY ${SORTS[sort]}
       LIMIT ${limit} OFFSET ${offset}`,
    )
    .all(...params);
  const credit = creditFor(
    ctx,
    rows.map((row) => row.email),
  );
  return json({
    total,
    users: rows.map((row) => {
      const c = credit.get(row.email)!;
      return {
        id: row.id,
        email: row.email,
        name: row.name,
        createdAt: row.created_at,
        lastSeenAt: row.last_seen_at,
        devices: {
          total: row.devices,
          desktop: row.desktops,
          mobile: row.mobiles,
        },
        credit: creditJson(ctx, c),
        // In one fixed order, so the same service sits in the same place on every row.
        services: SERVICES.filter((kind) =>
          c.services.some((s) => s.kind === kind),
        ),
      };
    }),
    next: offset + rows.length < total ? offset + rows.length : null,
  });
}

// ── GET /v1/admin/users/{id} ───────────────────────────────────────────────

export function userDetail(
  req: Request,
  ctx: Ctx,
  clientIp: string,
  id: string,
): Response {
  requireAdmin(req, ctx, clientIp);
  const user = ctx.store.userById(id);
  if (!user) throw notFound("No such user.");
  const db = ctx.store.database;
  const devices = db
    .query<DeviceRow, [string]>(
      `SELECT id, user_id, name, platform, kind, app_version, last_seen_at, created_at
       FROM devices WHERE user_id = ? ORDER BY last_seen_at DESC`,
    )
    .all(id);
  const credit = creditFor(ctx, [user.email]).get(user.email)!;
  const recent = db
    .query<
      {
        kind: string;
        model: string;
        units: number;
        unit: string;
        cost_usd: number;
        created_at: number;
      },
      [string, number]
    >(
      `SELECT kind, model, units, unit, cost_usd, created_at FROM free_usage
       WHERE subject = ? ORDER BY created_at DESC, id DESC LIMIT ?`,
    )
    .all(subjectFor(user.email), RECENT_REQUESTS);
  const feedback = db
    .query<{ n: number }, [string]>(
      "SELECT COUNT(*) AS n FROM feedback WHERE user_id = ?",
    )
    .get(id)!.n;
  return json({
    user: {
      id: user.id,
      email: user.email,
      name: user.name,
      createdAt: user.created_at,
      emailVerified: user.email_verified === 1,
      hasPassword: user.password_hash !== null,
      googleLinked: user.google_sub !== null,
      lastSeenAt: devices.length
        ? Math.max(...devices.map((d) => d.last_seen_at))
        : null,
    },
    devices: devices.map((d) => ({
      id: d.id,
      name: d.name,
      platform: d.platform,
      kind: d.kind,
      appVersion: d.app_version,
      online: ctx.hub.isOnline(d.id),
      lastSeenAt: d.last_seen_at,
      createdAt: d.created_at,
    })),
    credit: creditJson(ctx, credit),
    services: serviceJson(credit.services),
    recentRequests: recent.map((row) => ({
      service: row.kind,
      model: row.model,
      units: row.units,
      unit: row.unit,
      costUsd: round(row.cost_usd),
      at: row.created_at,
    })),
    feedback,
  });
}
