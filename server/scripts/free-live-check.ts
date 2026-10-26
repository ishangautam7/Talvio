/**
 * One tiny request per free-credit upstream, through a throwaway in-memory
 * server configured from the FREE_* environment: speech first, then the speech
 * is transcribed, then a short chat. Prints statuses, costs and the
 * transcript, never a key; fails if a key appears in any reply or log line.
 *
 *   bun scripts/free-live-check.ts                      (FREE_* already set)
 *   powershell -File scripts/live-check-free.ps1        (keys from this PC's Credential Manager)
 */
import { startServer } from "../src/app";
import { describeFree, freeFromEnv } from "../src/free/config";

const config = freeFromEnv(process.env);
console.log(`free credit: ${describeFree(config)}`);
const keys = [
  config.chat?.apiKey,
  config.stt?.apiKey,
  config.tts?.apiKey,
].filter((k): k is string => !!k);

const logs: string[] = [];
const app = startServer({
  port: 0,
  hostname: "127.0.0.1",
  databasePath: ":memory:",
  publicUrl: "http://127.0.0.1",
  google: null,
  trustProxy: false,
  secretsKey: Buffer.alloc(32, 7).toString("base64"),
  free: config,
  log: (line) => logs.push(line),
});
const base = `http://127.0.0.1:${app.port}`;
let leaked = false;
const check = (label: string, text: string) => {
  if (keys.some((k) => text.includes(k))) {
    leaked = true;
    console.log(`  ${label}: a key appeared in the reply!`);
  }
};

try {
  const signup = await fetch(`${base}/v1/auth/signup`, {
    method: "POST",
    body: JSON.stringify({
      email: "live-check@example.com",
      password: "live-check-password",
      device: { name: "live check", platform: "windows", kind: "desktop" },
    }),
  });
  const { token } = (await signup.json()) as { token: string };
  const auth = { authorization: `Bearer ${token}` };
  const remaining = (r: Response) => r.headers.get("x-talvio-credit-remaining");

  let mp3: ArrayBuffer | null = null;
  if (config.tts) {
    const r = await fetch(`${base}/v1/free/audio/speech`, {
      method: "POST",
      headers: { ...auth, "content-type": "application/json" },
      body: JSON.stringify({
        model: "talvio-free-speech",
        input: "Testing Talvio free credit.",
        voice: "alloy",
      }),
    });
    if (r.ok) mp3 = await r.arrayBuffer();
    else check("speech", await r.text());
    console.log(
      `speech:     ${r.status} ${r.headers.get("content-type")} ${mp3?.byteLength ?? 0} bytes, remaining ${remaining(r)}`,
    );
  }

  if (config.stt && mp3) {
    const form = new FormData();
    form.append("file", new Blob([mp3], { type: "audio/mpeg" }), "check.mp3");
    form.append("model", "talvio-free-transcribe");
    form.append("language", "en");
    const r = await fetch(`${base}/v1/free/audio/transcriptions`, {
      method: "POST",
      headers: auth,
      body: form,
    });
    const text = await r.text();
    check("transcribe", text);
    console.log(
      `transcribe: ${r.status} ${text.slice(0, 120)}, remaining ${remaining(r)}`,
    );
  }

  if (config.chat) {
    for (const stream of [false, true]) {
      const r = await fetch(`${base}/v1/free/chat/completions`, {
        method: "POST",
        headers: { ...auth, "content-type": "application/json" },
        body: JSON.stringify({
          model: "talvio-free-chat",
          stream,
          messages: [
            { role: "user", content: "Reply with the single word: ok" },
          ],
        }),
      });
      const text = await r.text();
      check("chat", text);
      const answer = stream
        ? `${text.split("\n").filter((l) => l.startsWith("data:")).length} SSE events, usage chunk ${text.includes('"usage"') ? "present" : "missing"}`
        : (() => {
            try {
              const j = JSON.parse(text);
              return `"${j.choices?.[0]?.message?.content ?? j.error?.message}" usage ${JSON.stringify(j.usage ?? null)}`;
            } catch {
              return text.slice(0, 120);
            }
          })();
      console.log(
        `chat${stream ? " (stream)" : ""}: ${r.status} ${answer}, remaining ${remaining(r)}`,
      );
    }
  }

  const balance = await fetch(`${base}/v1/free/balance`, { headers: auth });
  console.log(`balance:    ${await balance.text()}`);
  for (const line of logs) check("log", line);
  console.log(
    `upstream log lines: ${logs.filter((l) => l.startsWith("free ")).join(" | ") || "none"}`,
  );
  console.log(leaked ? "KEY LEAK DETECTED" : "no key in any reply or log line");
} finally {
  await app.stop();
}
if (leaked) process.exit(1);
