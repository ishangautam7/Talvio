import { Database } from "bun:sqlite";
import { randomToken, sha256 } from "./crypto";
import type { DeviceInfo } from "./validate";

/** A session token unused for this long is gone. */
const SESSION_IDLE_MS = 180 * 24 * 60 * 60 * 1000;
/** `last_seen_at` is written at most this often per device. */
export const TOUCH_EVERY_MS = 60 * 1000;

export interface UserRow {
  id: string;
  email: string;
  name: string | null;
  password_hash: string | null;
  /** 1 once Google proved the address; a password sign-up never verifies it. */
  email_verified: number;
  google_sub: string | null;
  created_at: number;
}

export interface DeviceRow {
  id: string;
  user_id: string;
  name: string;
  platform: string;
  kind: string;
  app_version: string | null;
  last_seen_at: number;
  created_at: number;
}

interface GoogleState {
  redirect_uri: string;
  challenge: string;
  expires_at: number;
}

interface LoginCode {
  user_id: string;
  challenge: string;
  expires_at: number;
}

const SCHEMA = `
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  name TEXT,
  password_hash TEXT,
  email_verified INTEGER NOT NULL,
  google_sub TEXT UNIQUE,
  created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE devices (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  platform TEXT NOT NULL,
  kind TEXT NOT NULL,
  app_version TEXT,
  last_seen_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX devices_by_user ON devices(user_id);
CREATE INDEX devices_by_last_seen ON devices(last_seen_at);
CREATE TABLE google_states (
  state_hash TEXT PRIMARY KEY,
  redirect_uri TEXT NOT NULL,
  challenge TEXT NOT NULL,
  expires_at INTEGER NOT NULL
) STRICT;
CREATE TABLE login_codes (
  code_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  challenge TEXT NOT NULL,
  expires_at INTEGER NOT NULL
) STRICT;
PRAGMA user_version = 1;
`;

const DEVICE_COLUMNS =
  "id, user_id, name, platform, kind, app_version, last_seen_at, created_at";

export class Store {
  private readonly db: Database;

  constructor(path: string) {
    this.db = new Database(path, { create: true, strict: true });
    this.db.run("PRAGMA journal_mode = WAL");
    this.db.run("PRAGMA foreign_keys = ON");
    this.db.run("PRAGMA busy_timeout = 5000");
    const { user_version } = this.db
      .query<{ user_version: number }, []>("PRAGMA user_version")
      .get()!;
    if (user_version === 0) this.db.transaction(() => this.db.run(SCHEMA))();
  }

  close(): void {
    this.db.close();
  }

  userById(id: string): UserRow | null {
    return this.db
      .query<UserRow, [string]>("SELECT * FROM users WHERE id = ?")
      .get(id);
  }

  userByEmail(email: string): UserRow | null {
    return this.db
      .query<UserRow, [string]>("SELECT * FROM users WHERE email = ?")
      .get(email);
  }

  userByGoogleSub(sub: string): UserRow | null {
    return this.db
      .query<UserRow, [string]>("SELECT * FROM users WHERE google_sub = ?")
      .get(sub);
  }

  /** Null when the email is already registered. */
  insertUser(
    user: Pick<UserRow, "email" | "name" | "password_hash" | "google_sub">,
    now: number,
  ): UserRow | null {
    return this.db
      .query<
        UserRow,
        [
          string,
          string,
          string | null,
          string | null,
          number,
          string | null,
          number,
        ]
      >(
        `INSERT INTO users (id, email, name, password_hash, email_verified, google_sub, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (email) DO NOTHING RETURNING *`,
      )
      .get(
        crypto.randomUUID(),
        user.email,
        user.name,
        user.password_hash,
        user.google_sub === null ? 0 : 1,
        user.google_sub,
        now,
      );
  }

  /**
   * Links a Google identity. `takeover` is for an account whose email was never
   * verified: Google's proof wins, so its password and every session go.
   */
  linkGoogle(userId: string, sub: string, takeover: boolean): void {
    this.db.transaction(() => {
      if (takeover) {
        this.db.run("UPDATE users SET password_hash = NULL WHERE id = ?", [
          userId,
        ]);
        this.db.run("DELETE FROM devices WHERE user_id = ?", [userId]);
      }
      this.db.run(
        "UPDATE users SET google_sub = ?, email_verified = 1 WHERE id = ?",
        [sub, userId],
      );
    })();
  }

  deleteUser(id: string): void {
    this.db.run("DELETE FROM users WHERE id = ?", [id]);
  }

  /** Signs a new device in; the token is returned once and only its hash is kept. */
  createDevice(
    userId: string,
    info: DeviceInfo,
    now: number,
  ): { device: DeviceRow; token: string } {
    const token = `zv1_${randomToken()}`;
    const device = this.db
      .query<
        DeviceRow,
        [
          string,
          string,
          string,
          string,
          string,
          string,
          string | null,
          number,
          number,
        ]
      >(
        `INSERT INTO devices (id, user_id, token_hash, name, platform, kind, app_version, last_seen_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING ${DEVICE_COLUMNS}`,
      )
      .get(
        crypto.randomUUID(),
        userId,
        sha256(token),
        info.name,
        info.platform,
        info.kind,
        info.appVersion,
        now,
        now,
      )!;
    return { device, token };
  }

  /** The live device a token belongs to, or null when unknown, revoked or idle too long. */
  deviceByToken(token: string, now: number): DeviceRow | null {
    return this.db
      .query<
        DeviceRow,
        [string, number]
      >(`SELECT ${DEVICE_COLUMNS} FROM devices WHERE token_hash = ? AND last_seen_at > ?`)
      .get(sha256(token), now - SESSION_IDLE_MS);
  }

  deviceExists(id: string): boolean {
    return (
      this.db
        .query<{ id: string }, [string]>("SELECT id FROM devices WHERE id = ?")
        .get(id) !== null
    );
  }

  listDevices(userId: string, now: number): DeviceRow[] {
    return this.db
      .query<
        DeviceRow,
        [string, number]
      >(`SELECT ${DEVICE_COLUMNS} FROM devices WHERE user_id = ? AND last_seen_at > ?`)
      .all(userId, now - SESSION_IDLE_MS);
  }

  touchDevice(id: string, at: number): void {
    this.db.run("UPDATE devices SET last_seen_at = ? WHERE id = ?", [at, id]);
  }

  /** False when no device with that id is on this account. */
  deleteDevice(id: string, userId: string): boolean {
    return (
      this.db.run("DELETE FROM devices WHERE id = ? AND user_id = ?", [
        id,
        userId,
      ]).changes > 0
    );
  }

  saveGoogleState(
    state: string,
    redirectUri: string,
    challenge: string,
    expiresAt: number,
  ): void {
    this.db.run(
      "INSERT INTO google_states (state_hash, redirect_uri, challenge, expires_at) VALUES (?, ?, ?, ?)",
      [sha256(state), redirectUri, challenge, expiresAt],
    );
  }

  /** Removes the state as it reads it, so it can be used once. */
  takeGoogleState(state: string): GoogleState | null {
    return this.db
      .query<
        GoogleState,
        [string]
      >("DELETE FROM google_states WHERE state_hash = ? RETURNING redirect_uri, challenge, expires_at")
      .get(sha256(state));
  }

  saveLoginCode(
    code: string,
    userId: string,
    challenge: string,
    expiresAt: number,
  ): void {
    this.db.run(
      "INSERT INTO login_codes (code_hash, user_id, challenge, expires_at) VALUES (?, ?, ?, ?)",
      [sha256(code), userId, challenge, expiresAt],
    );
  }

  /** Removes the code as it reads it: one redemption attempt, right or wrong. */
  takeLoginCode(code: string): LoginCode | null {
    return this.db
      .query<
        LoginCode,
        [string]
      >("DELETE FROM login_codes WHERE code_hash = ? RETURNING user_id, challenge, expires_at")
      .get(sha256(code));
  }

  /** Deletes idle sessions and expired Google states and login codes. */
  deleteExpired(now: number): void {
    this.db.transaction(() => {
      this.db.run("DELETE FROM devices WHERE last_seen_at <= ?", [
        now - SESSION_IDLE_MS,
      ]);
      this.db.run("DELETE FROM google_states WHERE expires_at <= ?", [now]);
      this.db.run("DELETE FROM login_codes WHERE expires_at <= ?", [now]);
    })();
  }
}

export function userJson(user: UserRow) {
  return {
    id: user.id,
    email: user.email,
    name: user.name,
    hasPassword: user.password_hash !== null,
    googleLinked: user.google_sub !== null,
    createdAt: user.created_at,
  };
}
