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

const HARNESS_NAMES: Record<HarnessInfo["harness"], string> = {
  codex: "Codex",
  claude: "Claude Code",
  cursor: "Cursor",
  cline: "Cline",
};

const PROVIDERS = [
  {
    id: "omniroute",
    name: "OmniRoute",
    hint: "Local router · one key, any model",
    placeholder: "sk-…",
    env: "OMNIROUTE_API_KEY",
    model: "gpt-5.6-sol-medium",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    hint: "Hosted gateway",
    placeholder: "sk-or-v1-…",
    env: "OPENROUTER_API_KEY",
    model: "openai/gpt-4.1-mini",
  },
  {
    id: "gemini",
    name: "Google Gemini",
    hint: "Google AI Studio · Gemini 4 Argon",
    placeholder: "AIza…",
    env: "GEMINI_API_KEY",
    model: "gemini-4-argon",
  },
] as const;

function storedKey(keys: KeyStatus | null, provider: string): boolean {
  if (!keys) return false;
  if (provider === "omniroute") return keys.omniroute;
  if (provider === "openrouter") return keys.openrouter;
  if (provider === "gemini") return keys.gemini;
  return false;
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
  const present = keys ? storedKey(keys, provider) : null;

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
      .harnesses()
      .then((next) => live && setHarnesses(next))
      .catch(() => live && setHarnesses([]));
    return () => {
      live = false;
    };
  }, []);

  const choose = async (id: string) => {
    if (!settings || settings.provider === id) return;
    // A model id belongs to one provider, so switching providers switches the default with
    // it; otherwise `gemini` would be asked for `gpt-5.6-sol-medium`.
    const entry = PROVIDERS.find((item) => item.id === id);
    const next = { ...settings, provider: id, model: entry?.model ?? settings.model };
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

  const probeHarnesses = async () => {
    setProbing(true);
    try {
      setHarnesses(await api.harnesses());
    } catch {
      setHarnesses([]);
    } finally {
      setProbing(false);
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
            const stored = storedKey(keys, item.id);
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
                : provider === "gemini"
                  ? "The native agent talks to Google's Gemini API directly. The key stays in the OS keychain and is read only when a turn starts."
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
            <button className="chip" onClick={() => void probeHarnesses()} disabled={probing}>
              {probing ? "Probing…" : "Probe again"}
            </button>
          </div>
          <p className="fine">
            Each external harness runs on its own API key, never a subscription login. The probe
            is cached in <code>adapters.json</code> beside <code>index.db</code>.
          </p>
          <div className="rows">
            {(harnesses ?? []).map((info) => {
              const needsKey = info.installed && info.auth_mode !== "api_key";
              return (
                <div key={info.harness} className="row">
                  <span className="row-text">
                    <span className="row-name">{HARNESS_NAMES[info.harness]}</span>
                    <span className="row-desc">
                      {info.installed
                        ? `${info.version ?? "unknown version"}${info.path ? ` · ${info.path}` : ""}`
                        : (info.install_hint ?? "not installed")}
                    </span>
                  </span>
                  <span
                    className={`pill ${
                      !info.installed ? "failed" : needsKey ? "cancelled" : "completed"
                    }`}
                  >
                    {!info.installed
                      ? "not installed"
                      : needsKey
                        ? `Needs API key · ${info.provider ?? "provider"}`
                        : info.provider
                          ? `API key · ${info.provider}`
                          : "API key"}
                  </span>
                </div>
              );
            })}
          </div>
          {harnesses !== null && harnesses.length === 0 ? (
            <p className="fine">
              No harnesses reported. The probe needs the desktop app; the browser preview has no
              Rust host to ask.
            </p>
          ) : null}
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
            The CLI reads the same keychain entry: <code>rigel keys set gemini</code> reads from
            stdin and never echoes the value. <code>rigel agent run --provider</code>{" "}
            switches between the router, OpenRouter and Gemini.
          </p>
        </div>
      </aside>
    </>
  );
}
