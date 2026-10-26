import type { Database } from "bun:sqlite";
import { sha256 } from "../crypto";

/**
 * The free credit ledger: one row per metered request, and a running total
 * per account, both written in one transaction.
 *
 * Balances belong to a *subject*, the SHA-256 of the account's email, not to
 * the account id: rows have no foreign key, so deleting an account and signing
 * up again with the same address finds the credit already used.
 *
 * Reservations (the worst-case cost of requests still upstream) live in this
 * process's memory, so a crash or restart can never leave credit held; the
 * server runs as one instance (see app.ts).
 *
 * Its tables are created here rather than as a numbered migration in db.ts, so
 * the feature carries its own schema.
 */
const SCHEMA = `
CREATE TABLE IF NOT EXISTS free_usage (
  id INTEGER PRIMARY KEY,
  subject TEXT NOT NULL,
  user_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  model TEXT NOT NULL,
  units REAL NOT NULL,
  unit TEXT NOT NULL,
  cost_usd REAL NOT NULL,
  created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS free_usage_by_subject ON free_usage(subject, created_at);
CREATE TABLE IF NOT EXISTS free_totals (
  subject TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  used_usd REAL NOT NULL,
  requests INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
) STRICT;
`;

export type FreeKind = "chat" | "transcribe" | "speech";

export interface Charge {
  kind: FreeKind;
  model: string;
  units: number;
  /** `tokens`, `seconds` or `characters`. */
  unit: string;
  costUsd: number;
}

export interface Balance {
  limitUsd: number;
  usedUsd: number;
  remainingUsd: number;
}

export const subjectFor = (email: string) =>
  sha256(`free:${email.trim().toLowerCase()}`);

/** Rounds to a millionth of a cent so sums stay readable. */
const round = (usd: number) => Math.round(usd * 1e8) / 1e8;

export class Reservation {
  private settled = false;

  constructor(
    private readonly ledger: FreeLedger,
    readonly subject: string,
    readonly userId: string,
    readonly amountUsd: number,
  ) {}

  /** Charges the real cost and releases the hold. Only the first settle or release counts. */
  settle(charge: Charge, now: number): void {
    if (this.settled) return;
    this.settled = true;
    this.ledger.release(this.subject, this.amountUsd);
    this.ledger.charge(this.subject, this.userId, charge, now);
  }

  /** Gives the whole hold back: the request cost nothing. */
  release(): void {
    if (this.settled) return;
    this.settled = true;
    this.ledger.release(this.subject, this.amountUsd);
  }
}

export class FreeLedger {
  private readonly held = new Map<string, number>();

  constructor(
    private readonly db: Database,
    readonly limitUsd: number,
  ) {
    db.transaction(() => db.run(SCHEMA))();
  }

  used(subject: string): number {
    return (
      this.db
        .query<
          { used_usd: number },
          [string]
        >("SELECT used_usd FROM free_totals WHERE subject = ?")
        .get(subject)?.used_usd ?? 0
    );
  }

  balance(subject: string): Balance {
    const usedUsd = round(this.used(subject));
    return {
      limitUsd: this.limitUsd,
      usedUsd,
      remainingUsd: round(Math.max(0, this.limitUsd - usedUsd)),
    };
  }

  /** What is left to spend, net of requests still running. */
  available(subject: string): number {
    return Math.max(
      0,
      this.limitUsd - this.used(subject) - (this.held.get(subject) ?? 0),
    );
  }

  /** Holds `amountUsd`, or null when that is more than is available. */
  reserve(
    subject: string,
    userId: string,
    amountUsd: number,
  ): Reservation | null {
    const available = this.available(subject);
    if (available <= 0 || amountUsd > available) return null;
    this.held.set(subject, (this.held.get(subject) ?? 0) + amountUsd);
    return new Reservation(this, subject, userId, amountUsd);
  }

  release(subject: string, amountUsd: number): void {
    const left = (this.held.get(subject) ?? 0) - amountUsd;
    if (left > 1e-12) this.held.set(subject, left);
    else this.held.delete(subject);
  }

  charge(subject: string, userId: string, charge: Charge, now: number): void {
    const cost = round(Math.max(0, charge.costUsd));
    this.db.transaction(() => {
      this.db.run(
        `INSERT INTO free_usage (subject, user_id, kind, model, units, unit, cost_usd, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
        [
          subject,
          userId,
          charge.kind,
          charge.model,
          charge.units,
          charge.unit,
          cost,
          now,
        ],
      );
      this.db.run(
        `INSERT INTO free_totals (subject, user_id, used_usd, requests, updated_at)
         VALUES (?, ?, ?, 1, ?)
         ON CONFLICT (subject) DO UPDATE SET
           user_id = excluded.user_id,
           used_usd = used_usd + excluded.used_usd,
           requests = requests + 1,
           updated_at = excluded.updated_at`,
        [subject, userId, cost, now],
      );
    })();
  }
}
