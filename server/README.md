# Talvio account server

The small server behind Talvio accounts. It signs people in (email and password,
or Google) on the desktop app and the phone app, keeps the list of devices on each
account, and relays messages between a person's own devices, which is what lets a
phone use the computer as its microphone and keyboard.

It stores **accounts, devices, and the provider API keys a person chose to keep
on their account** (encrypted; see "Stored API keys" below). Recordings and
transcripts pass through memory on their way from one device to another and are
never written down or logged.

Every route, message, limit and error code is defined in [PROTOCOL.md](PROTOCOL.md).
Change the protocol there first, then here and in both apps.

Built on Bun alone: `Bun.serve` for HTTP and WebSockets, `bun:sqlite` for storage,
`Bun.password` (argon2id) for passwords. There are no runtime dependencies.

## Run locally

```sh
cd server
bun install      # dev tooling only: Bun's types and TypeScript
bun run dev      # restarts on changes; `bun start` runs without watching
bun test
bunx tsc --noEmit
```

It listens on `http://localhost:8787` and keeps its database in `./data/`.

## How the apps find it

The apps don't hard-code this server's address. They read it from
`https://domain.gautamishan.com.np`, which must answer JSON naming the current
domain:

```json
{ "domain": "sync.example.com" }
```

The apps then use `https://sync.example.com`, so the server has to be reachable
over https at that name, for example through a tunnel or a reverse proxy in
front of port 8787:

```sh
bun run start                                  # http on :8787
cloudflared tunnel --url http://localhost:8787 # prints https://<random>.trycloudflare.com
```

When the domain changes, update what that address answers (and `PUBLIC_URL`).
Signed-in apps move to the new domain on their own, without signing in again.
Development builds also accept a plain `http://` URL there, such as
`http://192.168.1.5:8787` for a server on your network. The full rules are under
"Finding the server" in [PROTOCOL.md](PROTOCOL.md). Whoever controls that
address decides where people's sign-ins go, so keep it locked down.

## Configuration

| Variable                    | Default                  | Meaning                                                                                                                                           |
| --------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PORT`                      | `8787`                   | Port to listen on.                                                                                                                                |
| `HOST`                      | `0.0.0.0`                | Address to listen on.                                                                                                                             |
| `DATABASE_PATH`             | `./data/talvio.db`       | SQLite file. Its folder is created if missing. A `talvio.db` that does not exist yet, with a `zynveliq.db` beside it, opens that one.             |
| `PUBLIC_URL`                | `http://localhost:$PORT` | The address people's browsers reach the server at. Google sends them back to `$PUBLIC_URL/v1/auth/google/callback`.                               |
| `GOOGLE_CLIENT_ID`          | unset                    | Google OAuth client. Google sign-in is on only when both Google variables are set.                                                                |
| `GOOGLE_CLIENT_SECRET`      | unset                    | Stays on the server; the apps never see it.                                                                                                       |
| `TALVIO_SECRETS_KEY`        | unset                    | Base64 of 32 random bytes: the key that encrypts stored API keys. Unset uses (and on first start creates) `secrets.key` beside the database.      |
| `TRUST_PROXY`               | unset                    | `1` takes the client's address from the last `X-Forwarded-For` entry, for rate limiting. Set it only behind exactly one reverse proxy.            |
| `CLOUDFLARE_TURN_KEY_ID`    | unset                    | With `CLOUDFLARE_TURN_API_TOKEN`: a Cloudflare Realtime TURN key. The server asks Cloudflare for short-lived TURN credentials for each live view. |
| `CLOUDFLARE_TURN_API_TOKEN` | unset                    | The API token of that TURN key. Stays on the server.                                                                                              |
| `TURN_URLS`                 | unset                    | Instead of Cloudflare: your own TURN server (coturn with `use-auth-secret`), comma-separated `turn:` / `turns:` URLs.                             |
| `TURN_SECRET`               | unset                    | That server's `static-auth-secret`. Credentials follow the TURN REST scheme and last a day.                                                       |
| `FREE_*`                    | unset                    | The free credit upstreams, keys and prices. See "Free credit" below; it is off until they are set.                                                |

## Free credit

Every account gets `FREE_CREDIT_USD` ($1.00 by default) of AI usage paid with
the operator's own provider keys: cloud transcription, the assistant's answers
and spoken answers, one balance for all three. The apps reach it as an
OpenAI-compatible provider at `{server}/v1/free` with the session token as the
key; the routes, metering and errors are under "Free credit" in
[PROTOCOL.md](PROTOCOL.md).

The upstream keys live **only in the server's environment**: never in SQLite,
never in a log line (upstream failures are logged as `free chat: upstream
answered 401`, nothing more), never in a reply (an upstream's error message is
passed on only for a malformed request, with every key and upstream host
replaced by `[hidden]`). Each kind is on when its URL, key and model are all
set; with none set the feature is off (`/v1/free/balance` answers
`"enabled": false`, the rest `503 free_disabled`).

| Variable                        | Default                                 | Meaning                                                                                                                   |
| ------------------------------- | --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `FREE_CREDIT_USD`               | `1`                                     | Each account's allowance, in USD.                                                                                         |
| `FREE_RATE_PER_MINUTE`          | `60`                                    | Metered requests per minute per account (a spoken answer is one request per sentence).                                    |
| `FREE_CHAT_BASE_URL`            | unset                                   | OpenAI-compatible base for chat. An Azure OpenAI endpoint as pasted from the portal is moved to its `/openai/v1` surface. |
| `FREE_CHAT_API_KEY`             | unset                                   | Sent as `Authorization: Bearer`, and also as `api-key` to an Azure host.                                                  |
| `FREE_CHAT_MODEL`               | unset                                   | The model (on Azure, the deployment name) every chat request is sent to.                                                  |
| `FREE_CHAT_INPUT_USD_PER_MTOK`  | `0.20`                                  | Price per million prompt tokens.                                                                                          |
| `FREE_CHAT_OUTPUT_USD_PER_MTOK` | `1.20`                                  | Price per million completion tokens (reasoning tokens included).                                                          |
| `FREE_CHAT_MAX_OUTPUT_TOKENS`   | `8192`                                  | The most completion tokens one request may use.                                                                           |
| `FREE_STT_BASE_URL`             | unset                                   | OpenAI-compatible base for `/audio/transcriptions` (Groq, OpenAI, …).                                                     |
| `FREE_STT_API_KEY`              | unset                                   | Bearer key for it.                                                                                                        |
| `FREE_STT_MODEL`                | unset                                   | e.g. `whisper-large-v3-turbo`.                                                                                            |
| `FREE_STT_USD_PER_HOUR`         | `0.04`                                  | Price per hour of audio.                                                                                                  |
| `FREE_STT_MIN_SECONDS`          | `10`                                    | The shortest length one request is billed at (Groq's minimum).                                                            |
| `FREE_TTS_BASE_URL`             | unset                                   | `https://api.elevenlabs.io` (ElevenLabs) or an OpenAI-compatible base with `/audio/speech`.                               |
| `FREE_TTS_API_KEY`              | unset                                   | `xi-api-key` for ElevenLabs, Bearer otherwise.                                                                            |
| `FREE_TTS_KIND`                 | from the URL                            | `elevenlabs` or `openai`; only needed when the URL does not say.                                                          |
| `FREE_TTS_MODEL`                | `eleven_flash_v2_5` / `gpt-4o-mini-tts` | The speech model.                                                                                                         |
| `FREE_TTS_VOICE`                | ElevenLabs "Rachel" / `alloy`           | The voice when a request names none the upstream knows.                                                                   |
| `FREE_TTS_USD_PER_1K_CHARS`     | `0.05`                                  | Price per 1000 characters of speech.                                                                                      |

**The default prices** are the public list prices of the upstreams this server
is set up with (October 2026); set the variables when yours differ, since the
ledger charges whatever they say:

- Chat: GPT-5.6 Luna (OpenAI, and the same on Azure OpenAI), $0.20 per million
  input tokens and $1.20 per million output tokens.
- Transcription: Groq `whisper-large-v3-turbo`, $0.04 per hour of audio, billed
  at least 10 s per request.
- Speech: ElevenLabs Flash v2.5, $0.05 per 1000 characters.

At those prices $1 is roughly 25 hours of transcription, about a thousand short
assistant answers, or 20,000 characters (about 20 minutes) of
spoken answers; speech is what uses the credit up fastest.

**How a request is charged.** It first reserves its worst case (chat: the
prompt at about three characters per token, a fixed 1600 tokens per image,
plus its capped output; transcription: the WAV header's length, or the upload
at 16 kbit/s; speech: its characters) and is refused with `402
credit_exhausted` when that is more than is left; a chat request's output cap
is lowered to what the balance covers first, as long as that is at least 256
tokens. After the answer it is charged the real cost (the upstream's `usage`
tokens, its reported audio duration, the characters) and the rest of the hold
is released; a failed request is charged nothing. Holds live in memory, so a
restart never leaves credit stuck.

**The ledger** is two tables in the same SQLite file: `free_usage`, one row per
charged request (kind, model, units, cost, time), and `free_totals`, the
running total per account, both written in one transaction. Both are keyed by
a SHA-256 of the account's email rather than the account, so deleting an
account and signing up again with the same address does not renew the credit;
that hash (and the usage rows) stay after an account is deleted. To read it:

```sh
bun scripts/free-usage.ts ./data/talvio.db                       # locally
docker exec talvio-server bun scripts/free-usage.ts /data/talvio.db
```

which prints the totals, spend per kind and model over 30 days, and the top 20
accounts. Or ask SQLite directly:
`SELECT kind, SUM(cost_usd) FROM free_usage GROUP BY kind;`.

### Setting it up from the owner's PC

Three PowerShell scripts in `scripts/` read the keys the Talvio desktop app on
this Windows PC keeps in Credential Manager (and the non-secret settings
beside them). None of them prints a key or writes one to a local file.

1. `scripts\inspect-desktop-keys.ps1` lists which provider keys are there (as
   present or absent, with the vendor format they match), their base URLs,
   models and voices, to choose upstreams from.
2. `scripts\live-check-free.ps1` runs `scripts/free-live-check.ts` with the
   chosen upstreams: one tiny speech, transcription and chat request (plain and
   streamed) through an in-memory server, printing statuses, costs, the
   transcript, and whether a key turned up anywhere it should not.
3. `scripts\install-free-credit-env.ps1` writes them to the server's root-only
   env file (`/etc/talvio-server.env`, mode 600) over ssh, replacing any
   `FREE_*` lines already there, so it can be run again after a key changes.
   The values travel over ssh's stdin into `sudo tee -a`; `-DryRun` shows the
   variable names and non-secret values without connecting. Restart the server
   afterwards so it reads them; its start-up line then says
   `free credit $1 per account (chat …, transcribe …, speech …)`.

The upstreams it chooses (`Get-FreeCreditEnv` in `scripts/TalvioCredentials.ps1`):
chat on the Azure OpenAI deployment the desktop assistant uses
(`gpt-5.6-luna`), transcription on Groq `whisper-large-v3-turbo` with the
custom provider's Groq key, and speech on ElevenLabs `eleven_flash_v2_5` with
the assistant voice's key and voice.

## Live view across the internet (TURN)

A phone watching a window on its computer connects to it directly: always on
the same Wi-Fi, and across the internet through most home routers. Strict NATs,
which most mobile networks use, need a TURN server to relay the (still
end-to-end encrypted) video. `GET /v1/turn` hands both apps the servers to use:
a public STUN server always, and a TURN server when one of these is set:

- **Cloudflare** (no server to run): in the Cloudflare dashboard open
  **Realtime → TURN Server**, create a key, and set `CLOUDFLARE_TURN_KEY_ID` and
  `CLOUDFLARE_TURN_API_TOKEN`.
- **Your own coturn**: run it with `use-auth-secret` and
  `static-auth-secret=<secret>`, open UDP 3478 (and the relay port range), and
  set `TURN_URLS` and `TURN_SECRET`.

For a phone on mobile data the account server itself also has to be reachable
from the internet, since the handshake goes through it.

## Google sign-in

1. In the [Google Cloud Console](https://console.cloud.google.com/), open
   **APIs & Services → OAuth consent screen** and set up the app (name, support
   email; the scopes are `openid`, `email` and `profile`).
2. Open **Credentials → Create credentials → OAuth client ID** and pick the type
   **Web application**.
3. Under **Authorized redirect URIs**, add `{PUBLIC_URL}/v1/auth/google/callback`,
   for example `https://sync.example.com/v1/auth/google/callback`. It has to match
   `PUBLIC_URL` exactly.
4. Set `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` from the client you created
   and restart the server. `GET /v1/config` then answers `"google": true`.

The apps never talk to Google's token endpoint themselves: the server redeems
Google's code, then hands the app a one-time login code that only the app holding
the PKCE verifier can use.

## Deploy

```sh
docker build -t talvio-server server
docker run -d --name talvio-server --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -v talvio-data:/data \
  -e PUBLIC_URL=https://sync.example.com -e TRUST_PROXY=1 \
  -e GOOGLE_CLIENT_ID=... -e GOOGLE_CLIENT_SECRET=... \
  talvio-server
```

The image runs as a non-root user and keeps the database on the `/data` volume.

**A deployment from before the rename to Talvio** used the image and container
name `zynveliq-server`, the volume `zynveliq-data` and the file
`/data/zynveliq.db`. Keep that volume: mount it as before
(`-v zynveliq-data:/data`) and the server keeps opening `zynveliq.db`, since it
only starts a new `talvio.db` where neither file exists (it logs the old file
when it uses it). In the backup command, use your container's name and that
file.

- **Serve it over HTTPS.** Passwords and session tokens travel in requests, so put
  a TLS-terminating reverse proxy in front and never expose plain HTTP. The proxy
  must pass WebSocket upgrades on `/v1/ws` (Caddy does by default:
  `sync.example.com { reverse_proxy 127.0.0.1:8787 }`).
- **Run one instance.** Who is online, the socket routing and the rate limits live
  in the process's memory, so two instances would not see each other's devices.
  The account data itself is in SQLite.
- **Back up the database.** Losing it signs everyone out and deletes every account.
  The file is in WAL mode, so copy it with SQLite rather than `cp` while the server
  runs:

  ```sh
  docker exec talvio-server bun -e \
    'new (require("bun:sqlite").Database)("/data/talvio.db").run("VACUUM INTO ?", ["/data/backup.db"])'
  ```

  then move `/data/backup.db` off the machine.

## Stored API keys

`/v1/secrets` keeps a person's provider API keys (OpenAI, Azure, ElevenLabs, web
search, …) so they follow their account to every computer they sign in on. Each
value is encrypted with AES-256-GCM before it is written, with a fresh 12-byte
nonce per write and the account id and key name as associated data, so a
ciphertext cannot be copied onto another account or name. The database never
holds a value in plaintext, and the logs never contain one (only the name,
which is in the request path, and the status).

The encryption key comes from `TALVIO_SECRETS_KEY` when it is set. Otherwise it
is the file `secrets.key` in the database's folder (`./data/secrets.key`
locally, `/data/secrets.key` in the Docker image), created with 32 random bytes
on first start, mode 0600 where the OS has Unix modes. The start-up log names
which one is in use, never the key.

- **Back up the key with the database, but not in the same place.** Without it
  every stored API key is unreadable (`GET /v1/secrets` then fails with
  `internal`), and the apps have to be given their keys again. Anyone who has
  both the key and the database can read every stored API key.
- **Rotating the key is not supported yet**: there is no re-encryption tool.
  Changing the key makes the existing records unreadable.
- Over plain `http` (a server on your network without https in front) API keys
  travel in the clear, like passwords and tokens do; trust the network or put
  https in front.

Logs are one line per request (method, path, status, time) plus WebSocket connect
and disconnect counts. They never contain tokens, passwords, codes, email
addresses or message contents.

## Adding a phone ↔ computer feature

New kinds of phone ↔ computer messages, including the reserved `agent.*` family,
need no change here: they travel inside `relay` bodies, which the server never
reads, and their capability names pass through `hello` untouched. A new top-level
WebSocket message type would be one entry in the `handlers` table in
`src/hub.ts`.
