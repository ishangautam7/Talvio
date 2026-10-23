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
