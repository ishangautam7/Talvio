import { signedIn } from "./account";
import type { Ctx } from "./app";
import { pkceMatches, randomToken } from "./crypto";
import type { UserRow } from "./db";
import {
  ApiError,
  htmlPage,
  invalidRequest,
  isObject,
  readJson,
  redirect,
} from "./http";
import { deviceInfo } from "./validate";

export interface GoogleConfig {
  clientId: string;
  clientSecret: string;
  tokenUrl: string;
}

interface GoogleIdentity {
  sub: string;
  email: string;
  name: string | null;
}

export const GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token";
const GOOGLE_AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth";
const ISSUERS = new Set<unknown>([
  "accounts.google.com",
  "https://accounts.google.com",
]);
const STATE_TTL_MS = 10 * 60 * 1000;
const LOGIN_CODE_TTL_MS = 2 * 60 * 1000;
/**
 * The phone app's redirect. `zynveliq://` is the scheme from before the rename
 * to Talvio, kept so a phone app built before it can still sign in.
 */
const PHONE_REDIRECTS = new Set<string>([
  "talvio://auth/callback",
  "zynveliq://auth/callback",
]);
const LOOPBACK_REDIRECT = /^http:\/\/127\.0\.0\.1:([1-9]\d{3,4})\/callback$/;
const S256_CHALLENGE = /^[A-Za-z0-9_-]{43}$/;

const callbackUrl = (ctx: Ctx) => `${ctx.publicUrl}/v1/auth/google/callback`;

/** Only the redirects the apps use, matched exactly, so the code can go nowhere else. */
function isAllowedRedirect(uri: string): boolean {
  if (PHONE_REDIRECTS.has(uri)) return true;
  const port = Number(LOOPBACK_REDIRECT.exec(uri)?.[1]);
  return port >= 1024 && port <= 65535;
}

export function start(url: URL, ctx: Ctx): Response {
  const redirectUri = url.searchParams.get("redirect_uri") ?? "";
  const challenge = url.searchParams.get("code_challenge") ?? "";
  if (
    !isAllowedRedirect(redirectUri) ||
    !S256_CHALLENGE.test(challenge) ||
    url.searchParams.get("code_challenge_method") !== "S256"
  ) {
    return htmlPage(
      400,
      "This sign-in link is not valid. Start again from the Talvio app.",
    );
  }
  return toGoogle(ctx, redirectUri, challenge);
}

/** Where the admin dashboard's Google sign-in comes back to: the page itself. */
export const adminRedirect = (ctx: Ctx) => `${ctx.publicUrl}/admin`;

/**
 * The admin dashboard's Google sign-in (PROTOCOL.md, "Admin"): the same flow
 * as the apps', coming back to `/admin` with a code the page redeems at
 * `POST /v1/admin/google/exchange`.
 */
export function adminStart(url: URL, ctx: Ctx): Response {
  const challenge = url.searchParams.get("code_challenge") ?? "";
  if (
    !S256_CHALLENGE.test(challenge) ||
    url.searchParams.get("code_challenge_method") !== "S256"
  ) {
    return htmlPage(400, "This sign-in link is not valid. Start again.");
  }
  return toGoogle(ctx, adminRedirect(ctx), challenge);
}

function toGoogle(ctx: Ctx, redirectUri: string, challenge: string): Response {
  if (!ctx.google) return redirect(`${redirectUri}?error=google_unavailable`);
  const state = randomToken();
  ctx.store.saveGoogleState(
    state,
    redirectUri,
    challenge,
    ctx.now() + STATE_TTL_MS,
  );
  const params = new URLSearchParams({
    client_id: ctx.google.clientId,
    redirect_uri: callbackUrl(ctx),
    response_type: "code",
    scope: "openid email profile",
    prompt: "select_account",
    state,
  });
  return redirect(`${GOOGLE_AUTH_URL}?${params}`);
}

export async function callback(url: URL, ctx: Ctx): Promise<Response> {
  const flow = ctx.store.takeGoogleState(url.searchParams.get("state") ?? "");
  if (!flow || flow.expires_at <= ctx.now()) {
    return htmlPage(
      400,
      "This sign-in has expired. Start again from the Talvio app.",
    );
  }
  const back = (query: string) => redirect(`${flow.redirect_uri}?${query}`);
  if (!ctx.google) return back("error=google_unavailable");
  const error = url.searchParams.get("error");
  if (error)
    return back(
      `error=${error === "access_denied" ? "access_denied" : "google_failed"}`,
    );
  const identity = await googleIdentity(
    ctx.google,
    url.searchParams.get("code"),
    ctx,
  );
  if (!identity) return back("error=google_failed");
  const user = googleUser(ctx, identity);
  if (!user) return back("error=account_conflict");
  const code = randomToken();
  ctx.store.saveLoginCode(
    code,
    user.id,
    flow.challenge,
    ctx.now() + LOGIN_CODE_TTL_MS,
  );
  return back(`code=${code}`);
}

export async function exchange(req: Request, ctx: Ctx): Promise<Response> {
  const body = await readJson(req);
  const info = deviceInfo(body.device);
  return signedIn(ctx, redeemLoginCode(ctx, body), info);
}

/** The account a login code (with its PKCE verifier) signs in, or `invalid_code`; used once. */
export function redeemLoginCode(
  ctx: Ctx,
  body: Record<string, unknown>,
): UserRow {
  if (typeof body.code !== "string" || typeof body.codeVerifier !== "string") {
    throw invalidRequest("code and codeVerifier are required.");
  }
  const login = ctx.store.takeLoginCode(body.code);
  if (
    !login ||
    login.expires_at <= ctx.now() ||
    !pkceMatches(body.codeVerifier, login.challenge)
  ) {
    throw new ApiError(
      400,
      "invalid_code",
      "This sign-in code is not valid. Sign in again.",
    );
  }
  // Login codes are deleted with their user (ON DELETE CASCADE).
  return ctx.store.userById(login.user_id)!;
}

/**
 * Redeems Google's code at its token endpoint and reads the id_token from that
 * TLS response. No client ever hands us an id_token, so its signature needs no
 * JWKS check, but its audience, issuer and expiry still must match.
 */
async function googleIdentity(
  google: GoogleConfig,
  code: string | null,
  ctx: Ctx,
): Promise<GoogleIdentity | null> {
  if (!code) return null;
  let response: Response;
  try {
    response = await fetch(google.tokenUrl, {
      method: "POST",
      body: new URLSearchParams({
        grant_type: "authorization_code",
        code,
        client_id: google.clientId,
        client_secret: google.clientSecret,
        redirect_uri: callbackUrl(ctx),
      }),
      signal: AbortSignal.timeout(10_000),
    });
  } catch (error) {
    ctx.log(
      `google token request failed: ${error instanceof Error ? error.name : "error"}`,
    );
    return null;
  }
  if (!response.ok) {
    ctx.log(`google token request answered ${response.status}`);
    return null;
  }
  const claims = idTokenClaims(await response.json().catch(() => null));
  if (
    !claims ||
    !(
      claims.aud === google.clientId ||
      (Array.isArray(claims.aud) && claims.aud.includes(google.clientId))
    ) ||
    !ISSUERS.has(claims.iss) ||
    typeof claims.exp !== "number" ||
    claims.exp * 1000 <= ctx.now() ||
    (claims.email_verified !== true && claims.email_verified !== "true") ||
    typeof claims.sub !== "string" ||
    claims.sub === "" ||
    typeof claims.email !== "string"
  ) {
    ctx.log("google id_token rejected");
    return null;
  }
  const name =
    typeof claims.name === "string"
      ? [...claims.name.trim()].slice(0, 64).join("")
      : "";
  return {
    sub: claims.sub,
    email: claims.email.trim().toLowerCase(),
    name: name || null,
  };
}

function idTokenClaims(tokenResponse: unknown): Record<string, unknown> | null {
  const idToken = isObject(tokenResponse) ? tokenResponse.id_token : undefined;
  if (typeof idToken !== "string") return null;
  try {
    const claims: unknown = JSON.parse(
      Buffer.from(idToken.split(".")[1] ?? "", "base64url").toString("utf8"),
    );
    return isObject(claims) ? claims : null;
  } catch {
    return null;
  }
}

/**
 * By Google id; else by email, linking it; else a new account. Null when the
 * email's account is another Google account's.
 */
function googleUser(ctx: Ctx, identity: GoogleIdentity): UserRow | null {
  const linked = ctx.store.userByGoogleSub(identity.sub);
  if (linked) return linked;
  const existing = ctx.store.userByEmail(identity.email);
  if (existing) {
    // Addresses get handed to new people; the Google id is who someone is.
    if (existing.google_sub !== null) return null;
    // A password sign-up never proved the address, so whoever registered it
    // may not own it: Google's proof wins and their access ends here.
    const takeover = existing.email_verified === 0;
    ctx.store.linkGoogle(existing.id, identity.sub, takeover);
    if (takeover) ctx.hub.revokeUser(existing.id);
    return ctx.store.userById(existing.id)!;
  }
  // Nothing awaits between the lookup above and this insert, so the email is still free.
  return ctx.store.insertUser(
    {
      email: identity.email,
      name: identity.name,
      password_hash: null,
      google_sub: identity.sub,
    },
    ctx.now(),
  )!;
}
