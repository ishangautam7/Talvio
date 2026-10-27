import type { Ctx } from "./app";
import { randomToken } from "./crypto";
import { type DeviceRow, TOUCH_EVERY_MS, type UserRow, userJson } from "./db";
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
import {
  type DeviceInfo,
  deviceInfo,
  displayName,
  email,
  newPassword,
} from "./validate";

const BEARER = /^Bearer\s+(\S+)$/i;

/** The device whose bearer token signed this request. */
export function authenticate(req: Request, ctx: Ctx): DeviceRow {
  const token = BEARER.exec(req.headers.get("authorization") ?? "")?.[1];
  const now = ctx.now();
  const device = token ? ctx.store.deviceByToken(token, now) : null;
  if (!device) throw unauthorized();
  if (now - device.last_seen_at >= TOUCH_EVERY_MS) {
    ctx.store.touchDevice(device.id, now);
    device.last_seen_at = now;
  }
  return device;
}

/** Signs a new device in to `user` and answers with its AuthResponse. */
export function signedIn(
  ctx: Ctx,
  user: UserRow,
  info: DeviceInfo,
  status = 200,
): Response {
  const { device, token } = ctx.store.createDevice(user.id, info, ctx.now());
  ctx.hub.publishDevices(user.id);
  return json(
    { token, user: userJson(user), device: ctx.hub.device(device, device.id) },
    status,
  );
}

export async function signup(req: Request, ctx: Ctx): Promise<Response> {
  const body = await readJson(req);
  const info = deviceInfo(body.device);
  const name = displayName(body.name);
  const address = email(body.email);
  const password = newPassword(body.password);
  if (ctx.store.userByEmail(address)) throw emailTaken();
  const user = ctx.store.insertUser(
    {
      email: address,
      name,
      password_hash: await Bun.password.hash(password),
      google_sub: null,
    },
    ctx.now(),
  );
  // Null when a concurrent sign-up took the address while the password was hashing.
  if (!user) throw emailTaken();
  return signedIn(ctx, user, info, 201);
}

let dummyHash: Promise<string> | undefined;

export async function login(req: Request, ctx: Ctx): Promise<Response> {
  const body = await readJson(req);
  const info = deviceInfo(body.device);
  const user = await checkPassword(ctx, body.email, body.password);
  return signedIn(ctx, user, info);
}

/**
 * The account an email and password belong to, or `invalid_credentials`; shared
 * by the apps' login and the admin dashboard's, so both count failures alike.
 */
export async function checkPassword(
  ctx: Ctx,
  rawEmail: unknown,
  password: unknown,
): Promise<UserRow> {
  const address = email(rawEmail);
  if (typeof password !== "string")
    throw invalidRequest("password is required.");
  // Counted as a failure before the verify, so guesses sent at once cannot
  // all get one; a correct password gives the count back.
  const wait = ctx.loginFailures.take(address);
  if (wait > 0) throw rateLimited(wait);
  const user = ctx.store.userByEmail(address);
  // Checking against a throwaway hash keeps "no such account" as slow as "wrong password".
  const hash =
    user?.password_hash ??
    (await (dummyHash ??= Bun.password.hash(randomToken())));
  const matches = await Bun.password.verify(password, hash);
  // Read again: a Google takeover or the account's deletion may have landed during the verify.
  const current = matches && user ? ctx.store.userById(user.id) : null;
  if (!current || current.password_hash !== hash) {
    throw new ApiError(401, "invalid_credentials", "Wrong email or password.");
  }
  ctx.loginFailures.refund(address);
  return current;
}

export function logout(req: Request, ctx: Ctx): Response {
  const device = authenticate(req, ctx);
  signOut(ctx, device.user_id, device.id);
  return noContent();
}

export function me(req: Request, ctx: Ctx): Response {
  const device = authenticate(req, ctx);
  // A device row cannot outlive its user (ON DELETE CASCADE).
  const user = ctx.store.userById(device.user_id)!;
  return json({
    user: userJson(user),
    device: ctx.hub.device(device, device.id),
  });
}

export function deleteMe(req: Request, ctx: Ctx): Response {
  const device = authenticate(req, ctx);
  ctx.store.deleteUser(device.user_id);
  ctx.hub.revokeUser(device.user_id);
  return noContent();
}

export function listDevices(req: Request, ctx: Ctx): Response {
  const device = authenticate(req, ctx);
  const rows = ctx.store.listDevices(device.user_id, ctx.now());
  return json({ devices: ctx.hub.devices(rows, device.id) });
}

export function deleteDevice(req: Request, ctx: Ctx, id: string): Response {
  const device = authenticate(req, ctx);
  if (!signOut(ctx, device.user_id, id))
    throw notFound("No device with that id on this account.");
  return noContent();
}

function signOut(ctx: Ctx, userId: string, deviceId: string): boolean {
  if (!ctx.store.deleteDevice(deviceId, userId)) return false;
  ctx.hub.revokeDevice(deviceId);
  ctx.hub.publishDevices(userId);
  return true;
}

const emailTaken = () =>
  new ApiError(
    409,
    "email_taken",
    "An account with this email already exists.",
  );
