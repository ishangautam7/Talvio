/** At most `limit` events per sliding `windowMs`, counted per key. */
export class RateLimit {
  private readonly events = new Map<string, number[]>();

  constructor(
    private readonly limit: number,
    private readonly windowMs: number,
    private readonly now: () => number,
  ) {}

  /** Whole seconds until `key` may act again; 0 when it may act now. */
  retryAfter(key: string): number {
    const times = this.recent(key);
    if (times.length < this.limit) return 0;
    return Math.max(
      1,
      Math.ceil((times[0]! + this.windowMs - this.now()) / 1000),
    );
  }

  record(key: string): void {
    const times = this.recent(key);
    times.push(this.now());
    this.events.set(key, times);
  }

  /** Records the event unless the key is over its limit; returns `retryAfter`. */
  take(key: string): number {
    const wait = this.retryAfter(key);
    if (wait === 0) this.record(key);
    return wait;
  }

  /** Gives back the newest event, for an attempt `take` counted that turned out not to count. */
  refund(key: string): void {
    this.recent(key).pop();
  }

  /** Forgets keys with nothing left in their window, so memory follows recent traffic. */
  prune(): void {
    for (const key of this.events.keys()) this.recent(key);
  }

  private recent(key: string): number[] {
    const times = this.events.get(key) ?? [];
    const cutoff = this.now() - this.windowMs;
    while (times.length > 0 && times[0]! <= cutoff) times.shift();
    if (times.length === 0) this.events.delete(key);
    return times;
  }
}

/**
 * What an address is limited under: an IPv4 address whole, an IPv6 address by
 * its /64, since one home or server connection is handed a whole /64.
 */
export function addressKey(address: string): string {
  const mapped = /^::ffff:(\d+\.\d+\.\d+\.\d+)$/i.exec(address)?.[1];
  if (mapped) return mapped;
  if (!address.includes(":")) return address;
  let canonical: string;
  try {
    canonical = new URL(`http://[${address}]/`).hostname.slice(1, -1);
  } catch {
    return address;
  }
  const [head = [], tail] = canonical
    .split("::")
    .map((part) => (part ? part.split(":") : []));
  const groups = tail
    ? [...head, ...Array(8 - head.length - tail.length).fill("0"), ...tail]
    : head;
  return `${groups.slice(0, 4).join(":")}::/64`;
}
