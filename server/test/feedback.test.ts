import { afterEach, describe, expect, test } from "bun:test";
import {
  defuseMentions,
  feedbackGithubFromEnv,
  issueFor,
  stripControl,
} from "../src/feedback";
import { type Harness, harness, type Json } from "./helpers";

let h: Harness | undefined;
let github: ReturnType<typeof fakeGithub> | undefined;
afterEach(async () => {
  await h?.close();
  h = undefined;
  await github?.stop();
  github = undefined;
});

const ADMIN = "admin-token-for-the-tests-0123456789";
const PNG = Buffer.from([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13,
]);
const JPEG = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0, 16]);
const WEBP = Buffer.concat([
  Buffer.from("RIFF"),
  Buffer.from([16, 0, 0, 0]),
  Buffer.from("WEBPVP8 "),
]);

const shot = (bytes: Buffer, mediaType = "image/png") => ({
  mediaType,
  data: bytes.toString("base64"),
});

/** A PNG of exactly `size` bytes. */
const pngOf = (size: number) =>
  Buffer.concat([PNG, Buffer.alloc(size - PNG.length)]);

const send = (body: unknown, init: { token?: string; ip?: string } = {}) =>
  h!.request("POST", "/v1/feedback", {
    body,
    token: init.token,
    headers: init.ip ? { "x-forwarded-for": init.ip } : {},
  });

const list = (query = "", token = ADMIN) =>
  h!.request("GET", `/v1/admin/feedback${query}`, { token });

function fakeGithub(status = 201) {
  const issues: { path: string; auth: string | null; body: Json }[] = [];
  const server = Bun.serve({
    port: 0,
    hostname: "127.0.0.1",
    async fetch(req) {
      issues.push({
        path: new URL(req.url).pathname,
        auth: req.headers.get("authorization"),
        body: (await req.json()) as Json,
      });
      return Response.json({ number: 42 }, { status });
    },
  });
  return {
    apiUrl: `http://127.0.0.1:${server.port}`,
    issues,
    stop: () => server.stop(true),
  };
}

async function until(check: () => boolean) {
  const deadline = Date.now() + 3000;
  while (!check()) {
    if (Date.now() > deadline) throw new Error("timed out");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

describe("POST /v1/feedback", () => {
  test("stores feedback without a sign-in", async () => {
    h = await harness({ adminToken: ADMIN });
    const reply = await send({
      kind: "bug",
      message: "  The overlay hides behind Chrome.  ",
      email: " Me@Example.com ",
      system: "Talvio 2.3.0 · Windows 11 · x86_64 · nsis",
    });
    expect(reply.status).toBe(201);
    expect(typeof reply.body.id).toBe("string");

    const { body } = await list();
    expect(body.next).toBeNull();
    expect(body.feedback).toEqual([
      {
        id: reply.body.id,
        createdAt: h.now(),
        kind: "bug",
        message: "The overlay hides behind Chrome.",
        email: "me@example.com",
        system: "Talvio 2.3.0 · Windows 11 · x86_64 · nsis",
        user: null,
        device: null,
        screenshots: [],
      },
    ]);
  });

  test("only the message is required", async () => {
    h = await harness({ adminToken: ADMIN });
    expect((await send({ message: "hello there" })).status).toBe(201);
    expect(
      (await send({ message: "hi again", email: "", system: null })).status,
    ).toBe(201);
    const { body } = await list();
    expect(body.feedback.map((f: Json) => [f.kind, f.email, f.system])).toEqual(
      [
        [null, null, null],
        [null, null, null],
      ],
    );
  });

  test("a signed-in sender is linked to the account and device", async () => {
    h = await harness({ adminToken: ADMIN });
    const { token, user } = await h.signup();
    expect((await send({ message: "from my PC" }, { token })).status).toBe(201);
    // A token that is not valid is ignored, never refused.
    expect(
      (await send({ message: "stale session" }, { token: "zv1_nope" })).status,
    ).toBe(201);
    const { body } = await list();
    expect(body.feedback[1].user).toEqual({ id: user.id, email: user.email });
    expect(body.feedback[1].device).toEqual({
      name: "ISHAN-PC",
      platform: "windows",
      kind: "desktop",
    });
    expect(body.feedback[0].user).toBeNull();

    // Deleting the account keeps the feedback and drops the link.
    await h.request("DELETE", "/v1/me", { token });
    const after = await list();
    expect(after.body.feedback[1].message).toBe("from my PC");
    expect(after.body.feedback[1].user).toBeNull();
  });

  test("refuses what is out of shape", async () => {
    // Every attempt counts toward the limit, so each case comes from its own address.
    h = await harness({ trustProxy: true });
    const cases: [unknown, number, string][] = [
      [{}, 400, "invalid_request"],
      [{ message: "hi" }, 400, "invalid_request"],
      [{ message: "x".repeat(5001) }, 400, "invalid_request"],
      [{ message: "hello there", kind: "rant" }, 400, "invalid_request"],
      [{ message: "hello there", email: "nope" }, 400, "invalid_email"],
      [
        { message: "hello there", system: "s".repeat(301) },
        400,
        "invalid_request",
      ],
      [{ message: "hello there", screenshots: "x" }, 400, "invalid_request"],
      [
        { message: "hello there", screenshots: [shot(PNG, "image/gif")] },
        400,
        "invalid_request",
      ],
      [
        { message: "hello there", screenshots: [shot(JPEG, "image/png")] },
        400,
        "invalid_request",
      ],
      [
        {
          message: "hello there",
          screenshots: [{ mediaType: "image/png", data: "not base64!" }],
        },
        400,
        "invalid_request",
      ],
      [
        {
          message: "hello there",
          screenshots: [1, 2, 3, 4].map(() => shot(PNG)),
        },
        400,
        "invalid_request",
      ],
    ];
    for (const [index, [body, status, code]] of cases.entries()) {
      const reply = await send(body, { ip: `203.0.113.${index}` });
      expect([reply.status, reply.body.error?.code]).toEqual([status, code]);
    }
    expect((await send("not json")).body.error.code).toBe("invalid_request");
  });

  test("5,000 characters are counted as characters, not bytes", async () => {
    h = await harness();
    expect((await send({ message: "é".repeat(5000) })).status).toBe(201);
  });

  test("control characters are removed, line breaks kept", async () => {
    h = await harness({ adminToken: ADMIN });
    await send({
      message: "line one\r\nline\u0000 two‮\tend\u0007",
      system: "Talvio\n2.3",
    });
    const [entry] = (await list()).body.feedback;
    expect(entry.message).toBe("line one\nline two\tend");
    expect(entry.system).toBe("Talvio2.3");
  });

  test("screenshots are stored and served back to the owner", async () => {
    h = await harness({ adminToken: ADMIN });
    const reply = await send({
      message: "three formats",
      screenshots: [
        shot(PNG),
        shot(JPEG, "image/jpeg"),
        shot(WEBP, "image/webp"),
      ],
    });
    expect(reply.status).toBe(201);
    const [entry] = (await list()).body.feedback;
    expect(entry.screenshots).toEqual([
      {
        mediaType: "image/png",
        bytes: PNG.length,
        url: `https://zv.test/v1/admin/feedback/${reply.body.id}/screenshots/0`,
      },
      {
        mediaType: "image/jpeg",
        bytes: JPEG.length,
        url: `https://zv.test/v1/admin/feedback/${reply.body.id}/screenshots/1`,
      },
      {
        mediaType: "image/webp",
        bytes: WEBP.length,
        url: `https://zv.test/v1/admin/feedback/${reply.body.id}/screenshots/2`,
      },
    ]);
    const response = await fetch(
      `${h.base}/v1/admin/feedback/${reply.body.id}/screenshots/2`,
      { headers: { authorization: `Bearer ${ADMIN}` } },
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("image/webp");
    expect(response.headers.get("x-content-type-options")).toBe("nosniff");
    expect(Buffer.from(await response.arrayBuffer())).toEqual(WEBP);

    const missing = await h.request(
      "GET",
      `/v1/admin/feedback/${reply.body.id}/screenshots/3`,
      { token: ADMIN },
    );
    expect(missing.status).toBe(404);
    const anonymous = await h.request(
      "GET",
      `/v1/admin/feedback/${reply.body.id}/screenshots/0`,
    );
    expect(anonymous.status).toBe(401);
  });

  test("screenshots are capped at 1 MiB each and 3 MiB together", async () => {
    h = await harness();
    const mib = 1024 * 1024;
    expect(
      (await send({ message: "at the cap", screenshots: [shot(pngOf(mib))] }))
        .status,
    ).toBe(201);
    const over = await send({
      message: "over the cap",
      screenshots: [shot(pngOf(mib + 1))],
    });
    expect([over.status, over.body.error.code]).toEqual([
      413,
      "payload_too_large",
    ]);
    const three = await send({
      message: "three at the cap",
      screenshots: [shot(pngOf(mib)), shot(pngOf(mib)), shot(pngOf(mib))],
    });
    expect(three.status).toBe(201);
  });

  test("a body over 4.5 MiB is refused before it is read whole", async () => {
    h = await harness();
    const reply = await send({
      message: "huge",
      padding: "x".repeat(5 * 1024 * 1024),
    });
    expect([reply.status, reply.body.error.code]).toEqual([
      413,
      "payload_too_large",
    ]);
  });

  test("rate-limited per address and for the whole server", async () => {
    h = await harness({ trustProxy: true });
    for (let i = 0; i < 10; i++) {
      expect(
        (await send({ message: `report ${i}` }, { ip: "198.51.100.7" })).status,
      ).toBe(201);
    }
    const limited = await send({ message: "one more" }, { ip: "198.51.100.7" });
    expect([limited.status, limited.body.error.code]).toEqual([
      429,
      "rate_limited",
    ]);
    expect(Number(limited.headers.get("retry-after"))).toBeGreaterThan(0);
    // Another address still gets through, and an hour later this one does too.
    expect(
      (await send({ message: "elsewhere" }, { ip: "198.51.100.8" })).status,
    ).toBe(201);
    h.advance(60 * 60 * 1000 + 1);
    expect(
      (await send({ message: "an hour on" }, { ip: "198.51.100.7" })).status,
    ).toBe(201);

    h.advance(60 * 60 * 1000 + 1);
    for (let i = 0; i < 200; i++) {
      await send({ message: `flood ${i}` }, { ip: `10.0.${i >> 3}.${i}` });
    }
    const global = await send({ message: "too many" }, { ip: "192.0.2.99" });
    expect(global.status).toBe(429);
  });
});

describe("the owner's feedback routes", () => {
  test("do not exist without ADMIN_TOKEN", async () => {
    h = await harness();
    await send({ message: "hello there" });
    for (const [method, path] of [
      ["GET", "/v1/admin/feedback"],
      ["GET", "/v1/admin/feedback/x/screenshots/0"],
      ["DELETE", "/v1/admin/feedback/x"],
    ] as const) {
      const reply = await h.request(method, path, { token: "anything" });
      expect([reply.status, reply.body.error.code]).toEqual([404, "not_found"]);
    }
  });

  test("need the admin token, not a session token", async () => {
    h = await harness({ adminToken: ADMIN });
    const { token } = await h.signup();
    expect((await list("", token)).status).toBe(401);
    expect((await h.request("GET", "/v1/admin/feedback")).status).toBe(401);
    expect((await list()).status).toBe(200);
  });

  test("wrong tokens are rate-limited", async () => {
    h = await harness({ adminToken: ADMIN });
    for (let i = 0; i < 10; i++)
      expect((await list("", "wrong")).status).toBe(401);
    // Locked out even with the right token, until the window passes.
    expect((await list()).status).toBe(429);
    h.advance(15 * 60 * 1000 + 1);
    expect((await list()).status).toBe(200);
  });

  test("pages newest first", async () => {
    h = await harness({ adminToken: ADMIN });
    for (let i = 0; i < 5; i++) {
      await send({ message: `report ${i}` });
      h.advance(1000);
    }
    const first = await list("?limit=2");
    expect(first.body.feedback.map((f: Json) => f.message)).toEqual([
      "report 4",
      "report 3",
    ]);
    const second = await list(`?limit=2&before=${first.body.next}`);
    expect(second.body.feedback.map((f: Json) => f.message)).toEqual([
      "report 2",
      "report 1",
    ]);
    const third = await list(`?limit=2&before=${second.body.next}`);
    expect(third.body.feedback.map((f: Json) => f.message)).toEqual([
      "report 0",
    ]);
    expect(third.body.next).toBeNull();
    expect((await list("?limit=0")).status).toBe(400);
    expect((await list("?limit=101")).status).toBe(400);
    expect((await list("?before=abc")).status).toBe(400);
  });

  test("delete an entry and its screenshots", async () => {
    h = await harness({ adminToken: ADMIN });
    const { body } = await send({
      message: "delete me",
      screenshots: [shot(PNG)],
    });
    const gone = await h.request("DELETE", `/v1/admin/feedback/${body.id}`, {
      token: ADMIN,
    });
    expect(gone.status).toBe(204);
    expect((await list()).body.feedback).toEqual([]);
    const shotAfter = await h.request(
      "GET",
      `/v1/admin/feedback/${body.id}/screenshots/0`,
      { token: ADMIN },
    );
    expect(shotAfter.status).toBe(404);
    const again = await h.request("DELETE", `/v1/admin/feedback/${body.id}`, {
      token: ADMIN,
    });
    expect(again.status).toBe(404);
  });
});

describe("GitHub issues", () => {
  test("are off unless both variables are set", () => {
    expect(feedbackGithubFromEnv({})).toBeNull();
    expect(feedbackGithubFromEnv({ FEEDBACK_GITHUB_TOKEN: "t" })).toBeNull();
    expect(
      feedbackGithubFromEnv({
        FEEDBACK_GITHUB_TOKEN: "t",
        FEEDBACK_GITHUB_REPO: "owner/repo",
      }),
    ).toEqual({ token: "t", repo: "owner/repo" });
    expect(() =>
      feedbackGithubFromEnv({
        FEEDBACK_GITHUB_TOKEN: "t",
        FEEDBACK_GITHUB_REPO: "https://github.com/owner/repo",
      }),
    ).toThrow();
  });

  test("each entry is filed, saying how many screenshots the server holds", async () => {
    github = fakeGithub();
    h = await harness({
      feedbackGithub: {
        token: "ghp_x",
        repo: "owner/repo",
        apiUrl: github.apiUrl,
      },
    });
    const reply = await send({
      kind: "idea",
      message: "Dark overlay\nIt would be nice, @octocat.",
      email: "me@example.com",
      system: "Talvio 2.3.0 · Windows 11",
      screenshots: [shot(PNG), shot(JPEG, "image/jpeg")],
    });
    expect(reply.status).toBe(201);
    await until(() => github!.issues.length === 1);
    const [issue] = github.issues;
    expect(issue!.path).toBe("/repos/owner/repo/issues");
    expect(issue!.auth).toBe("Bearer ghp_x");
    expect(issue!.body.title).toBe("[idea] Dark overlay");
    expect(issue!.body.body).toContain("@​octocat");
    expect(issue!.body.body).toContain("System: Talvio 2.3.0 · Windows 11");
    expect(issue!.body.body).toContain("Reply to: me@example.com");
    expect(issue!.body.body).toContain("2 screenshots stored on the server.");
    expect(issue!.body.body).not.toContain("/v1/admin/");
  });

  test("a failed filing does not fail the feedback", async () => {
    github = fakeGithub(500);
    h = await harness({
      adminToken: ADMIN,
      feedbackGithub: { token: "t", repo: "o/r", apiUrl: github.apiUrl },
    });
    expect((await send({ message: "still stored" })).status).toBe(201);
    await until(() => github!.issues.length === 1);
    expect((await list()).body.feedback).toHaveLength(1);
  });
});

describe("helpers", () => {
  test("a long first line is cut for the title", () => {
    const { title, body } = issueFor({
      kind: null,
      message: `\n${"word ".repeat(40)}\nmore`,
      email: null,
      system: null,
      screenshots: [],
    });
    expect([...title].length).toBe(80);
    expect(title.endsWith("…")).toBe(true);
    expect(body).not.toContain("Reply to");
    expect(body).not.toContain("screenshot");
  });

  test("mentions and control characters", () => {
    expect(defuseMentions("ping @someone and a@b.co")).toBe(
      "ping @​someone and a@​b.co",
    );
    expect(stripControl("a\r\nb\u0085c", true)).toBe("a\nbc");
    expect(stripControl("a\nb", false)).toBe("ab");
  });
});
