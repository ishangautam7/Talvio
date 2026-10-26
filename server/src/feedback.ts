import type { Ctx } from "./app";
import { sha256 } from "./crypto";
import type { FeedbackInsert, FeedbackRow } from "./db";
import {
  invalidRequest,
  isObject,
  json,
  noContent,
  notFound,
  payloadTooLarge,
  rateLimited,
  readJson,
  unauthorized,
} from "./http";
import type { RateLimit } from "./rateLimit";
import { email as emailAddress } from "./validate";

/**
 * Feedback from the apps' feedback forms (PROTOCOL.md, "Feedback"): stored in
 * this server's database, read by the owner through the admin routes, and
 * optionally filed as a GitHub issue. No sign-in is needed to send it.
 */

export const MIN_MESSAGE_CHARS = 3;
export const MAX_MESSAGE_CHARS = 5000;
const MAX_SYSTEM_CHARS = 300;
export const MAX_SCREENSHOTS = 3;
export const MAX_SCREENSHOT_BYTES = 1024 * 1024;
export const MAX_SCREENSHOTS_TOTAL_BYTES = 3 * 1024 * 1024;
/** Three screenshots at the total cap in base64, plus the text around them. */
export const MAX_FEEDBACK_BODY_BYTES = 4.5 * 1024 * 1024;
const KINDS = new Set<unknown>(["bug", "idea", "question"]);
const MEDIA_TYPES = new Set<unknown>(["image/png", "image/jpeg", "image/webp"]);
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

/** Where an entry is also filed as an issue, if anywhere. */
export interface FeedbackGithub {
  token: string;
  /** `owner/name`. */
  repo: string;
  /** Defaults to GitHub's API; tests point it elsewhere. */
  apiUrl?: string;
}

export interface FeedbackLimits {
  perIp: RateLimit;
  global: RateLimit;
  adminFailures: RateLimit;
}

/** The GitHub settings from the environment, or null to file nothing. */
export function feedbackGithubFromEnv(
  env: Record<string, string | undefined>,
): FeedbackGithub | null {
  const token = env.FEEDBACK_GITHUB_TOKEN?.trim();
  const repo = env.FEEDBACK_GITHUB_REPO?.trim();
  if (!token || !repo) return null;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) {
    throw new Error(
      `FEEDBACK_GITHUB_REPO must be owner/name, not "${env.FEEDBACK_GITHUB_REPO}".`,
    );
  }
  return { token, repo };
}

/** Code points. */
const length = (text: string) => [...text].length;

/**
 * Removes control characters (C0, DEL, C1) and the bidi overrides that can make
 * stored text read differently from what it is. `keepLines` keeps line breaks
 * and tabs, with CRLF and CR made LF.
 */
export function stripControl(text: string, keepLines: boolean): string {
  const normalized = text.replace(/\r\n?/g, "\n");
  return normalized.replace(
    // eslint-disable-next-line no-control-regex
    /[\u0000-\u001f\u007f-\u009f‪-‮⁦-⁩]/g,
    (char) => (keepLines && (char === "\n" || char === "\t") ? char : ""),
  );
}

/** The image format the leading bytes say a file is, if it is one we accept. */
export function sniffImage(bytes: Uint8Array): string | null {
  const starts = (sig: number[], at = 0) =>
    bytes.length >= at + sig.length && sig.every((b, i) => bytes[at + i] === b);
  if (starts([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))
    return "image/png";
  if (starts([0xff, 0xd8, 0xff])) return "image/jpeg";
  if (starts([0x52, 0x49, 0x46, 0x46]) && starts([0x57, 0x45, 0x42, 0x50], 8))
    return "image/webp";
  return null;
}

function optionalText(
  value: unknown,
  field: string,
  max: number,
): string | null {
  if (value === undefined || value === null) return null;
  if (typeof value !== "string")
    throw invalidRequest(`${field} must be a string.`);
  const text = stripControl(value, false).trim();
  if (length(text) > max)
    throw invalidRequest(`${field} must be at most ${max} characters.`);
  return text || null;
}

function screenshots(value: unknown): FeedbackInsert["screenshots"] {
  if (value === undefined || value === null) return [];
  if (!Array.isArray(value) || value.length > MAX_SCREENSHOTS) {
    throw invalidRequest(`screenshots must be a list of at most 3.`);
  }
  let total = 0;
  return value.map((item) => {
    if (!isObject(item) || !MEDIA_TYPES.has(item.mediaType)) {
      throw invalidRequest(
        "Each screenshot needs a mediaType of image/png, image/jpeg or image/webp.",
      );
    }
    if (typeof item.data !== "string" || !BASE64.test(item.data)) {
      throw invalidRequest("Each screenshot's data must be base64.");
    }
    // Checked before decoding, so a huge string is never decoded.
    if (item.data.length > Math.ceil(MAX_SCREENSHOT_BYTES / 3) * 4) {
      throw payloadTooLarge("A screenshot is larger than 1 MiB.");
    }
    const data = new Uint8Array(Buffer.from(item.data, "base64"));
    if (data.length > MAX_SCREENSHOT_BYTES) {
      throw payloadTooLarge("A screenshot is larger than 1 MiB.");
    }
    total += data.length;
    if (total > MAX_SCREENSHOTS_TOTAL_BYTES) {
      throw payloadTooLarge("The screenshots are larger than 3 MiB together.");
    }
    if (sniffImage(data) !== item.mediaType) {
      throw invalidRequest("A screenshot is not the image its mediaType says.");
    }
    return { mediaType: item.mediaType as string, data };
  });
}

/** The stored form of a request body, or the error that refuses it. */
export function parseFeedback(
  body: Record<string, unknown>,
): Omit<FeedbackInsert, "userId" | "device"> {
  const kind = body.kind ?? null;
  if (kind !== null && !KINDS.has(kind)) {
    throw invalidRequest("kind must be bug, idea or question.");
  }
  if (typeof body.message !== "string")
    throw invalidRequest("message is required.");
  const message = stripControl(body.message, true).trim();
  if (
    length(message) < MIN_MESSAGE_CHARS ||
    length(message) > MAX_MESSAGE_CHARS
  ) {
    throw invalidRequest("message must be 3 to 5000 characters.");
  }
  const rawEmail =
    typeof body.email === "string"
      ? stripControl(body.email, false).trim()
      : body.email;
  const email =
    rawEmail === undefined || rawEmail === null || rawEmail === ""
      ? null
      : emailAddress(rawEmail);
  return {
    kind: kind as string | null,
    message,
    email,
    system: optionalText(body.system, "system", MAX_SYSTEM_CHARS),
    screenshots: screenshots(body.screenshots),
  };
}

const BEARER = /^Bearer\s+(\S+)$/i;

export async function sendFeedback(
  req: Request,
  ctx: Ctx,
  clientIp: string,
): Promise<Response> {
  const { perIp, global } = ctx.feedbackLimits;
  const wait = Math.max(perIp.retryAfter(clientIp), global.retryAfter("all"));
  if (wait > 0) throw rateLimited(wait);
  perIp.record(clientIp);
  global.record("all");

  const entry = parseFeedback(await readJson(req, MAX_FEEDBACK_BODY_BYTES));
  // A session is a bonus here, never a requirement: a stale token is ignored.
  const token = BEARER.exec(req.headers.get("authorization") ?? "")?.[1];
  const device = token ? ctx.store.deviceByToken(token, ctx.now()) : null;
  const id = ctx.store.insertFeedback(
    {
      ...entry,
      userId: device?.user_id ?? null,
      device: device && {
        name: device.name,
        platform: device.platform,
        kind: device.kind,
      },
    },
    ctx.now(),
  );
  ctx.log(
    `feedback ${id}: ${entry.kind ?? "-"}, ${length(entry.message)} chars, ${entry.screenshots.length} screenshot(s)${device ? ", signed in" : ""}`,
  );
  if (ctx.feedbackGithub) {
    const github = ctx.feedbackGithub;
    void fileIssue(github, entry).then(
      (number) => ctx.log(`feedback ${id}: filed as ${github.repo}#${number}`),
      (error) =>
        ctx.log(
          `feedback ${id}: could not file a GitHub issue: ${error instanceof Error ? error.message : String(error)}`,
        ),
    );
  }
  return json({ id }, 201);
}

/** Keeps `@name` from notifying anyone when the text lands on GitHub. */
export const defuseMentions = (text: string) =>
  text.replace(/@(?=[A-Za-z0-9])/g, "@​");

export function issueFor(entry: Omit<FeedbackInsert, "userId" | "device">): {
  title: string;
  body: string;
} {
  const firstLine = entry.message.split("\n").find((line) => line.trim()) ?? "";
  const chars = [...firstLine.trim()];
  const head =
    chars.length > 80 ? `${chars.slice(0, 79).join("")}…` : chars.join("");
  const title = entry.kind ? `[${entry.kind}] ${head}` : head;
  const lines = [defuseMentions(entry.message), "", "---"];
  if (entry.system) lines.push(`System: ${defuseMentions(entry.system)}`);
  if (entry.email) lines.push(`Reply to: ${entry.email}`);
  const shots = entry.screenshots.length;
  if (shots > 0) {
    lines.push(
      `${shots} screenshot${shots === 1 ? "" : "s"} stored on the server.`,
    );
  }
  lines.push("Sent from the in-app feedback form.");
  return { title: defuseMentions(title), body: lines.join("\n") };
}

async function fileIssue(
  github: FeedbackGithub,
  entry: Omit<FeedbackInsert, "userId" | "device">,
): Promise<number> {
  const api = github.apiUrl ?? "https://api.github.com";
  const response = await fetch(`${api}/repos/${github.repo}/issues`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${github.token}`,
      accept: "application/vnd.github+json",
      "content-type": "application/json",
      "user-agent": "talvio-server",
      "x-github-api-version": "2022-11-28",
    },
    body: JSON.stringify(issueFor(entry)),
    signal: AbortSignal.timeout(15_000),
  });
  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
  const { number } = (await response.json()) as { number: number };
  return number;
}

// ── The owner's routes ─────────────────────────────────────────────────────

const sameSecret = (a: string, b: string) => sha256(a) === sha256(b);

/** Throws unless the request carries the admin token; `not_found` when none is set. */
function admin(req: Request, ctx: Ctx, clientIp: string): void {
  if (!ctx.adminToken) throw notFound();
  const failures = ctx.feedbackLimits.adminFailures;
  const wait = failures.retryAfter(clientIp);
  if (wait > 0) throw rateLimited(wait);
  const token = BEARER.exec(req.headers.get("authorization") ?? "")?.[1];
  // Compared as hashes, so the time taken says nothing about the token.
  if (!token || !sameSecret(token, ctx.adminToken)) {
    failures.record(clientIp);
    throw unauthorized();
  }
}

function entryJson(
  ctx: Ctx,
  row: FeedbackRow,
  shots: { n: number; media_type: string; bytes: number }[],
) {
  return {
    id: row.id,
    createdAt: row.created_at,
    kind: row.kind,
    message: row.message,
    email: row.email,
    system: row.system,
    user:
      row.user_id && row.user_email
        ? { id: row.user_id, email: row.user_email }
        : null,
    device: row.device_name
      ? {
          name: row.device_name,
          platform: row.device_platform,
          kind: row.device_kind,
        }
      : null,
    screenshots: shots.map((shot) => ({
      mediaType: shot.media_type,
      bytes: shot.bytes,
      url: `${ctx.publicUrl}/v1/admin/feedback/${row.id}/screenshots/${shot.n}`,
    })),
  };
}

export function listFeedback(
  req: Request,
  ctx: Ctx,
  url: URL,
  clientIp: string,
): Response {
  admin(req, ctx, clientIp);
  const limitParam = url.searchParams.get("limit");
  const limit = limitParam === null ? 50 : Number(limitParam);
  if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
    throw invalidRequest("limit must be 1 to 100.");
  }
  const beforeParam = url.searchParams.get("before");
  const before = beforeParam === null ? null : Number(beforeParam);
  if (before !== null && !Number.isInteger(before)) {
    throw invalidRequest("before must be a cursor from next.");
  }
  const { rows, screenshots } = ctx.store.listFeedback(limit, before);
  return json({
    feedback: rows.map((row) =>
      entryJson(
        ctx,
        row,
        screenshots.filter((shot) => shot.feedback_id === row.id),
      ),
    ),
    next: rows.length === limit ? rows.at(-1)!.seq : null,
  });
}

export function feedbackScreenshot(
  req: Request,
  ctx: Ctx,
  clientIp: string,
  id: string,
  n: number,
): Response {
  admin(req, ctx, clientIp);
  const shot = ctx.store.feedbackScreenshot(id, n);
  if (!shot) throw notFound("No such screenshot.");
  return new Response(shot.data, {
    headers: {
      "content-type": shot.media_type,
      "cache-control": "no-store",
      "content-security-policy": "default-src 'none'",
      "x-content-type-options": "nosniff",
    },
  });
}

export function deleteFeedback(
  req: Request,
  ctx: Ctx,
  clientIp: string,
  id: string,
): Response {
  admin(req, ctx, clientIp);
  if (!ctx.store.deleteFeedback(id)) throw notFound("No such feedback.");
  return noContent();
}
