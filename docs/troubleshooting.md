# Troubleshooting cookbook (roadmap 198)

Each entry starts from the line the app actually emits, then names the cause and the fix. The
commands in the last column are the ones that reproduce or verify the shape on this machine.

## 401: a credential from another vendor

```
HTTP/1.1 401 invalid x-api-key
the stored `groq` credential looks like an Anthropic key (`sk-ant-…`); Groq does not accept it.
```

**Cause.** The key stored for one provider belongs to another, or a stale key was pasted after
switching providers. The local shape guard names the mismatch before a request; if an unknown
shape reaches the API, the provider's own 401 is preserved.

**Fix.** Store the key under the provider that issued it: Settings → Providers, or
`rigel keys set <provider>`. The second sentence names the family found and the provider that
refuses it. An unrecognised shape is allowed on purpose; the endpoint is the authority on its own
token.

## 401: the local router refused the request

```
HTTP/1.1 401 Unauthorized
{"error":{"message":"Authentication required","type":"invalid_api_key","code":"invalid_api_key"}}
```

**Cause.** `/v1/models` is the router's authenticated surface: a missing or rejected OmniRoute key
is refused even though `/v1/chat/completions` and `/v1/responses` may still answer on loopback. A
turn that works therefore does not prove the model roster is authenticated.

**Fix.** Store the router key with `rigel keys set omniroute` (Settings → Providers), or set
`OMNIROUTE_API_KEY` for CI. If the window points at another router, set `RIGEL_OMNIROUTE_URL` to
its `/v1` base; this 401 names the wrong key, not a missing server.

## 429: rate limited

```
provider returned 429: {"error":{"type":"rate_limit_error","message":"..."}}
HTTP/1.1 429 Too Many Requests
Retry-After: 12
```

**Cause.** The provider's rate limit or shared free pool is exhausted. Anthropic and OpenRouter
retry a 429 once, honour `Retry-After`, and feed the wait back into the shared limiter so other
callers on the same provider wait too.

**Fix.** Wait for the retry or switch models/providers. If a shared free pool is exhausted, the
free-mode row says `rate_limited`; the next candidate is tried rather than retrying the same pool
forever.

## App Check: the attestation endpoint refused this client

```
the attestation endpoint refused this client (403 Forbidden); set the App Check client token for this installation
```

**Cause.** App Check is configured and the endpoint is reachable, but this installation has no
client token, or the stored token is not on the endpoint's allowlist. This is a refusal, not an
outage: the offline path says `the attestation endpoint is not reachable: …` instead.

**Fix.** Store the client token for this installation (Settings → Account → App Check, or the
`docs/cloud.md` setup command), or add the token's fingerprint to the attestation service's
allowlist. A 401 names a rejected stored token:

```
the attestation endpoint refused this client (401 Unauthorized); the stored client token is not on its allowlist
```

A reachable endpoint that answers in the wrong shape produces a different contract error; fix the
service, not the token:

```
the attestation endpoint must answer with {"token": "…", "expires_at": <unix seconds>}: …
```

## MCP: the handshake did not complete

```
demo closed before initialize
demo refused initialize
demo did not answer tools/list in time
```

**Cause.** A project `.mcp.json` server exited, rejected the MCP `initialize` request, or did not
answer `tools/list` inside the bounded handshake. A server that does not answer is not silently
trusted, and no tool from it is offered.

**Fix.** Run the command from `.mcp.json` yourself and check that it speaks MCP over stdio. Remove
or correct the server entry, then try the approval card again. The card shows the real tool list
only after a successful handshake.

## Migration refusal: the database is newer than the build

```
schema version 99 is newer than this build knows about (newest: 17)
```

**Cause.** A newer Rigel build migrated the project database, and the current binary is older.
Opening it half-read would corrupt rows, so the store refuses instead.

**Fix.** Run the newer build that created the schema, or restore a backup made before the upgrade.
A renamed migration is refused for the same reason; never edit an applied migration.

## Updater: the manifest signature does not verify

```
the manifest signature does not verify against this key
```

**Cause.** The release manifest changed after it was signed, the signing key does not match the
bundled public key, or the download was altered. The signature covers the version, URL, SHA-256
and optional MSI fields, so any edit fails this check.

**Fix.** Use a manifest signed by the release key, or rebuild the binary with the matching
`RIGEL_UPDATE_PUBKEY`. Auto-install never starts an artifact whose hash does not match the signed
manifest; do not bypass this line.
