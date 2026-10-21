import { useEffect, useState } from "react";

import {
  api,
  type HarnessInfo,
  type KeyStatus,
  type OmnirouteStatus,
  type Settings,
} from "./api";

interface Props {
  keys: KeyStatus | null;
  onKeys: (status: KeyStatus) => void;
  onNotice: (notice: string) => void;
}

const PROVIDERS = [
  {
    id: "omniroute",
    name: "OmniRoute",
    hint: "Local router · one key, any model",
    placeholder: "sk-…",
    env: "OMNIROUTE_API_KEY",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    hint: "Hosted gateway",
    placeholder: "sk-or-v1-…",
    env: "OPENROUTER_API_KEY",
  },
] as const;

const HARNESS_KEYS = [
  {
    id: "openai",
    harness: "Codex",
    name: "OpenAI",
    env: "OPENAI_API_KEY",
    placeholder: "sk-…",
    note: "Direct Codex only; on the local router Codex uses the OmniRoute entry.",
  },
  {
    id: "anthropic",
    harness: "Claude Code",
    name: "Anthropic",
    env: "ANTHROPIC_API_KEY",
    placeholder: "sk-ant-…",
    note: "Claude Code receives this key at spawn.",
  },
  {
    id: "cursor",
    harness: "Cursor",
    name: "Cursor",
    env: "CURSOR_API_KEY",
    placeholder: "key-…",
    note: "The Cursor CLI receives this key at spawn.",
  },
] as const;

type HarnessKey = (typeof HARNESS_KEYS)[number];

const HARNESS_LABELS: Record<HarnessInfo["harness"], string> = {
  native: "Rigel",
  codex: "Codex",
  claude: "Claude Code",
  cursor: "Cursor",
  cline: "Cline",
};

/**
 * The auth line the handoff requires: a harness that is not on an API key says so, and names the
 * keychain entry that would fix it. A subscription is never presented as usable (D21).
 */
function authCopy(info: HarnessInfo): string {
  if (info.auth_mode === "api_key") {
    return info.provider ? `API key · ${info.provider}` : "API key";
  }
  if (info.auth_mode === "subscription") {
    return info.provider
      ? `Needs API key · ${info.provider} — the vendor login is never reused`
      : "Needs API key — the vendor login is never reused";
  }
  if (info.provider) return `Needs API key · ${info.provider}`;
  return "Provider keys arrive with the Cline adapter (P4.4)";
}

function HarnessRow({ info }: { info: HarnessInfo }) {
  return (
    <div className="harness-row">
      <div className="harness-row-head">
        <strong>{HARNESS_LABELS[info.harness]}</strong>
        <span className={`pill ${info.installed ? "completed" : "cancelled"}`}>
          {info.installed ? (info.version ?? "installed") : "not installed"}
        </span>
      </div>
      {info.installed ? (
        <code className="harness-path">{info.path ?? "on PATH"}</code>
      ) : (
        <p className="fine">
          Install: <code>{info.install_hint ?? "not on PATH"}</code>
        </p>
      )}
      <p className={`fine ${info.auth_mode === "api_key" ? "" : "needs-key"}`}>
        {authCopy(info)}
      </p>
    </div>
  );
}

function HarnessKeyRow({
  item,
  present,
  onKeys,
  onNotice,
}: {
  item: HarnessKey;
  present: boolean;
  onKeys: (status: KeyStatus) => void;
  onNotice: (notice: string) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    const key = value.trim();
    if (!key) return;
    setBusy(true);
    setError(null);
    try {
      onKeys(await api.setKey(item.id, key));
      setValue("");
      setConfirmRemove(false);
      onNotice(`${item.name} key stored for ${item.harness}.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      onKeys(await api.deleteKey(item.id));
      setConfirmRemove(false);
      onNotice(`${item.name} key removed.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="harness-key-row">
      <div className="harness-key-label">
        <strong>{item.harness}</strong>
        <span>
          {present ? `${item.name} key set` : `Needs API key · ${item.id}`}
        </span>
      </div>
      <input
        type="password"
        aria-label={`${item.name} API key`}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            void save();
          }
        }}
        placeholder={item.placeholder}
        autoComplete="off"
        spellCheck={false}
        disabled={busy}
      />
      <div className="settings-actions">
        <button className="primary" onClick={() => void save()} disabled={busy || !value.trim()}>
          Save
        </button>
        {present ? (
          confirmRemove ? (
            <>
              <button className="chip danger" onClick={() => void remove()} disabled={busy}>
                Confirm delete
              </button>
              <button className="chip" onClick={() => setConfirmRemove(false)} disabled={busy}>
                Cancel
              </button>
            </>
          ) : (
            <button className="chip danger" onClick={() => setConfirmRemove(true)} disabled={busy}>
              Delete
            </button>
          )
        ) : null}
      </div>
      <p className="fine">
        {error ? (
          error
        ) : (
          <>
            Write-only entry <code>rigel/{item.id}</code>. Headless: <code>{item.env}</code>.
            <br />
            {item.note}
          </>
        )}
      </p>
    </div>
  );
}

/**
 * Providers and keys. The renderer can store a key and ask whether one exists; it never
 * receives the value back. That is enforced by the Tauri commands, not by this form.
 *
 * OmniRoute has a second axis OpenRouter does not: the router is a process on this machine,
 * so this screen reports whether it is answering and can start it.
 */
export default function SettingsView({ keys, onKeys, onNotice }: Props) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [server, setServer] = useState<OmnirouteStatus | null>(null);
  const [serverBusy, setServerBusy] = useState(false);
  const [harnesses, setHarnesses] = useState<HarnessInfo[] | null>(null);
  const [probing, setProbing] = useState(false);

  const provider = settings?.provider ?? "omniroute";
  const providerInfo = PROVIDERS.find((item) => item.id === provider) ?? PROVIDERS[0];
  const present = keys ? (provider === "omniroute" ? keys.omniroute : keys.openrouter) : null;

  useEffect(() => {
    let live = true;
    api
      .getSettings()
      .then((next) => live && setSettings(next))
      .catch(() => live && setSettings(null));
    api
      .omnirouteStatus()
      .then((next) => live && setServer(next))
      .catch(() => live && setServer(null));
    api
      .harnessProbe(false)
      .then((next) => live && setHarnesses(next))
      .catch(() => live && setHarnesses(null));
    return () => {
      live = false;
    };
  }, []);

  const refreshHarnesses = async (force = false) => {
    setProbing(force);
    try {
      setHarnesses(await api.harnessProbe(force));
    } catch {
      setHarnesses(null);
    } finally {
      setProbing(false);
    }
  };

  // A stored or deleted key changes the auth line immediately; the probe only needs to
  // re-classify, not re-run the binaries.
  const applyKeys = (status: KeyStatus) => {
    onKeys(status);
    void refreshHarnesses();
  };

  const choose = async (id: string) => {
    if (!settings || settings.provider === id) return;
    const next = { ...settings, provider: id };
    setSettings(next);
    setValue("");
    setConfirmRemove(false);
    try {
      setSettings(await api.saveSettings(next));
      onNotice(`Agent provider set to ${id}.`);
      if (id === "omniroute") {
        setServer(await api.omnirouteStatus());
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const refreshServer = async () => {
    try {
      setServer(await api.omnirouteStatus());
    } catch {
      setServer(null);
    }
  };

  const startServer = async () => {
    setServerBusy(true);
    setError(null);
    try {
      const status = await api.omnirouteStart();
      setServer(status);
      onNotice(`OmniRoute is answering at ${status.origin}.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
      await refreshServer();
    } finally {
      setServerBusy(false);
    }
  };

  const save = async () => {
    const key = value.trim();
    if (!key) return;
    setBusy(true);
    setError(null);
    try {
      const status = await api.setKey(provider, key);
      onKeys(status);
      setValue("");
      setConfirmRemove(false);
      onNotice(
        `${providerInfo.name} key stored in the OS keychain. This window will not read it back.`,
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    setError(null);
    try {
      const status = await api.deleteKey(provider);
      onKeys(status);
      setConfirmRemove(false);
      onNotice(`${providerInfo.name} key removed from the OS keychain.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <aside className="list-pane settings-nav">
        <div className="list-head">
          <div className="list-title">
            <h1>Settings</h1>
            <span>local</span>
          </div>
        </div>
        <div className="rows">
          {PROVIDERS.map((item) => {
            const stored = keys
              ? item.id === "omniroute"
                ? keys.omniroute
                : keys.openrouter
              : false;
            return (
              <button
                key={item.id}
                role="option"
                aria-selected={provider === item.id}
                className={`row${provider === item.id ? " on" : ""}`}
                onClick={() => void choose(item.id)}
              >
                <span className="row-text">
                  <span className="row-name">{item.name}</span>
                  <span className="row-desc">{item.hint}</span>
                </span>
                <span className={`status-dot ${stored ? "idle" : "failed"}`} />
              </button>
            );
          })}
        </div>
        <div className="roster-foot">
          <span className={`status-dot ${present ? "idle" : "failed"}`} />
          <span>{present ? "key stored" : "no key"}</span>
          <span className="grow" />
          <span className="mono">OS keychain</span>
        </div>
      </aside>

      <main className="detail session-main settings-main">
        <header className="detail-head">
          <div>
            <p className="eyebrow">RIGEL · SETTINGS</p>
            <h2>Providers and keys</h2>
            <p className="lede">
              {provider === "omniroute"
                ? "The native agent talks to OmniRoute on this machine. The router is started automatically when a turn needs it."
                : "The native agent talks to OpenRouter directly. The key is stored in the OS keychain and read only when a turn starts."}
            </p>
          </div>
        </header>

        <p className="notice" role="status">
          {error
            ? error
            : present
              ? "A key is stored. Rigel can confirm it exists; it never reads it back into this window."
              : "No key yet. Paste one below — it is never shown again."}
        </p>

        {provider === "omniroute" ? (
          <section className="settings-section">
            <div className="settings-head">
              <h3>Router</h3>
              <span className={`pill ${server?.running ? "completed" : "failed"}`}>
                {server ? (server.running ? "running" : "offline") : "checking"}
              </span>
            </div>
            <p className="fine">
              {server?.running
                ? `OmniRoute is answering at ${server.origin}.`
                : server
                  ? `Nothing is answering at ${server.origin}. Start it here; the next turn would start it anyway.`
                  : "Could not probe the router."}
            </p>
            <div className="settings-actions">
              <button
                className="primary"
                onClick={() => void startServer()}
                disabled={serverBusy || server?.running === true}
              >
                {serverBusy ? "Starting…" : server?.running ? "Running" : "Start server"}
              </button>
              <button className="chip" onClick={() => void refreshServer()} disabled={serverBusy}>
                Check again
              </button>
            </div>
            <p className="fine">
              Runs <code>omniroute serve --daemon --no-open</code> and waits for{" "}
              <code>/api/health</code>. Override the address with{" "}
              <code>RIGEL_OMNIROUTE_URL</code>.
            </p>
          </section>
        ) : null}

        <section className="settings-section">
          <div className="settings-head">
            <h3>{providerInfo.name}</h3>
            <span className={`pill ${present ? "completed" : "cancelled"}`}>
              {present ? "key stored" : "no key"}
            </span>
          </div>
          <label className="key-field">
            <span>API key</span>
            <input
              type="password"
              value={value}
              onChange={(event) => setValue(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void save();
                }
              }}
              placeholder={providerInfo.placeholder}
              autoComplete="off"
              spellCheck={false}
              disabled={busy}
            />
          </label>
          <div className="settings-actions">
            <button
              className="primary"
              onClick={() => void save()}
              disabled={busy || !value.trim()}
            >
              {busy ? "Saving…" : "Save key"}
            </button>
            {present ? (
              confirmRemove ? (
                <>
                  <button className="chip danger" onClick={() => void remove()} disabled={busy}>
                    Confirm remove
                  </button>
                  <button
                    className="chip"
                    onClick={() => setConfirmRemove(false)}
                    disabled={busy}
                  >
                    Cancel
                  </button>
                </>
              ) : (
                <button
                  className="chip danger"
                  onClick={() => setConfirmRemove(true)}
                  disabled={busy}
                >
                  Remove key
                </button>
              )
            ) : null}
          </div>
          <p className="fine">
            Stored as <code>rigel/{providerInfo.id}</code> in the OS keychain. Headless runs can
            set <code>{providerInfo.env}</code> instead; Rigel never writes a key to a config
            file.
          </p>
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Harnesses</h3>
            <button
              className="chip"
              onClick={() => void refreshHarnesses(true)}
              disabled={probing}
            >
              {probing ? "Probing\u2026" : "Probe again"}
            </button>
          </div>
          <p className="fine">
            Detected from <code>--version</code> and cached in <code>adapters.json</code>, which is
            re-probed when a binary changes.
          </p>
          {harnesses ? (
            <div className="harness-list">
              {harnesses.map((info) => (
                <HarnessRow key={info.harness} info={info} />
              ))}
            </div>
          ) : (
            <p className="fine">The probe is unavailable in this window.</p>
          )}
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Harness keys</h3>
            <span className="pill cancelled">API keys only</span>
          </div>
          <p className="fine">
            Each harness receives only its own key when Rigel starts the process. Vendor logins and
            subscriptions are never reused.
          </p>
          <div className="harness-key-list">
            {HARNESS_KEYS.map((item) => (
              <HarnessKeyRow
                key={item.id}
                item={item}
                present={keys?.[item.id] ?? false}
                onKeys={applyKeys}
                onNotice={onNotice}
              />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <h3>Model</h3>
          <p className="fine">
            The native agent's default is <code>{settings?.model ?? "gpt-5.6-sol-medium"}</code>;
            override it per turn with <code>--model</code>. Subagents are pinned to the cheap
            allowlist and cannot be pointed at an expensive model.
          </p>
        </section>
      </main>

      <aside className="inspector">
        <div>
          <h3>Key safety</h3>
          <p className="fine">
            Keys never enter the transcript, the session database or a config file. Saving clears
            the field; the value you typed is the only copy the window held.
          </p>
        </div>
        <div>
          <h3>Headless</h3>
          <p className="fine">
            The CLI reads the same keychain entries. <code>rigel keys set openai</code> and the
            other provider commands read from stdin and never echo the value.
          </p>
        </div>
      </aside>
    </>
  );
}
