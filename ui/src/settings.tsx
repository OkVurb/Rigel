import { useEffect, useState } from "react";

import {
  api,
  type HarnessInfo,
  type KeyStatus,
  type OmnirouteStatus,
  type RuleSource,
  type SafetyView,
  type Settings,
  type SpendReport,
  type UpdateCheck,
  type UpdateStatus,
} from "./api";
import { ModelControls } from "./modelpicker";
import { ensurePermission } from "./notify";

interface Props {
  keys: KeyStatus | null;
  project: string | null;
  session: string | null;
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
  {
    id: "gemini",
    harness: "Gemini CLI",
    name: "Google AI Studio",
    env: "GEMINI_API_KEY",
    placeholder: "AIza…",
    note: "One key serves the Gemini ACP harness and the native agent's gemini provider.",
  },
] as const;

type HarnessKey = (typeof HARNESS_KEYS)[number];

const HARNESS_LABELS: Record<HarnessInfo["harness"], string> = {
  native: "Rigel",
  codex: "Codex",
  claude: "Claude Code",
  cursor: "Cursor",
  cline: "Cline",
  gemini: "Gemini CLI",
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
function dollarsLabel(micros: number | null): string {
  return micros === null ? "no cap" : dollars(micros);
}

function dollars(micros: number): string {
  const sign = micros < 0 ? "-" : "";
  const value = Math.abs(micros);
  return `${sign}$${Math.floor(value / 1_000_000)}.${String(Math.floor((value % 1_000_000) / 100)).padStart(4, "0")}`;
}

export default function SettingsView({ keys, project, session, onKeys, onNotice }: Props) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [server, setServer] = useState<OmnirouteStatus | null>(null);
  const [serverBusy, setServerBusy] = useState(false);
  const [harnesses, setHarnesses] = useState<HarnessInfo[] | null>(null);
  const [probing, setProbing] = useState(false);
  const [diagnostics, setDiagnostics] = useState<{ path: string; body: string } | null>(null);
  const [rules, setRules] = useState<RuleSource[]>([]);
  const [exporting, setExporting] = useState(false);
  const [spend, setSpend] = useState<SpendReport | null>(null);
  const [safety, setSafety] = useState<SafetyView | null>(null);
  const [writableInput, setWritableInput] = useState("");
  const [dailyInput, setDailyInput] = useState("");
  const [safetyBusy, setSafetyBusy] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [updateCheck, setUpdateCheck] = useState<UpdateCheck | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);

  const provider = settings?.provider ?? "omniroute";
  const providerInfo = PROVIDERS.find((item) => item.id === provider) ?? PROVIDERS[0];
  const present = keys ? (provider === "omniroute" ? keys.omniroute : keys.openrouter) : null;

  useEffect(() => {
    if (!project) {
      setRules([]);
      return;
    }
    let live = true;
    api
      .rulesSources(project)
      .then((next) => live && setRules(next))
      .catch(() => live && setRules([]));
    return () => {
      live = false;
    };
  }, [project]);

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
    api
      .updateStatus()
      .then((next) => live && setUpdateStatus(next))
      .catch(() => live && setUpdateStatus(null));
    if (project) {
      api
        .spendReport(project)
        .then((next) => live && setSpend(next))
        .catch(() => live && setSpend(null));
      api
        .safetyGet(project)
        .then((next) => {
          if (!live) return;
          setSafety(next);
          setWritableInput(String(next.writable_agents));
          setDailyInput(
            next.daily_cap_micros === null
              ? ""
              : String(Math.round(next.daily_cap_micros / 10_000)),
          );
        })
        .catch(() => live && setSafety(null));
    }
    return () => {
      live = false;
    };
  }, [project]);

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

  const exportDiagnostics = async () => {
    if (!project) {
      onNotice("Open a project first — diagnostics include its database check and last turn.");
      return;
    }
    setExporting(true);
    try {
      const bundle = await api.exportDiagnostics(project, session);
      setDiagnostics(bundle);
      await navigator.clipboard?.writeText(bundle.body).catch(() => undefined);
      onNotice(`Diagnostics written to ${bundle.path}`);
    } catch (failure) {
      onNotice(failure instanceof Error ? failure.message : "Could not assemble diagnostics.");
    } finally {
      setExporting(false);
    }
  };

  // D113/D116: one save writes both knobs, because they are read together everywhere.
  const saveSafety = async (clearDaily = false) => {
    if (!project) {
      onNotice("Open a project first — the safety settings live in its database.");
      return;
    }
    setSafetyBusy(true);
    try {
      const count = Number.parseInt(writableInput, 10);
      const dailyDollars = clearDaily ? null : Number.parseFloat(dailyInput);
      const next = await api.safetySet(project, session ?? "", {
        writableAgents: Number.isFinite(count) ? Math.max(1, Math.min(16, count)) : undefined,
        dailyCapCents:
          !clearDaily && dailyDollars !== null && Number.isFinite(dailyDollars)
            ? Math.max(1, Math.round(dailyDollars * 100))
            : undefined,
        clearDailyCap: clearDaily,
      });
      setSafety(next);
      setWritableInput(String(next.writable_agents));
      setDailyInput(
        next.daily_cap_micros === null ? "" : String(Math.round(next.daily_cap_micros / 10_000)),
      );
      onNotice(
        clearDaily
          ? "Daily cap turned off. Session caps still apply."
          : `Safety saved: ${next.writable_agents} writable agent(s), ${dollarsLabel(next.daily_cap_micros)} a day.`,
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setSafetyBusy(false);
    }
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

  const saveModel = async (model: string, effort: string, provider?: string) => {
    if (!settings) return;
    const next = { ...settings, model, effort, ...(provider ? { provider } : {}) };
    setSettings(next);
    try {
      setSettings(await api.saveSettings(next));
      onNotice(
        `Default model: ${model} · ${effort} effort${provider ? ` · ${provider}` : ""}.`,
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const saveUpdateSwitch = async (
    field: "enabled" | "auto_install",
    enabled: boolean,
  ) => {
    if (!updateStatus) return;
    setUpdateBusy(true);
    try {
      const next = await api.updateSet(
        field === "enabled" ? enabled : updateStatus.enabled,
        field === "auto_install" ? enabled : updateStatus.auto_install,
        null,
      );
      setUpdateStatus(next);
      if (!next.enabled) setUpdateCheck(null);
      onNotice(
        field === "enabled"
          ? enabled
            ? "Rigel will check GitHub once at launch and on demand."
            : "Update checks are off; no request is made."
          : enabled
            ? "A signed update will download and install when found."
            : "Updates will wait for an Install click.",
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setUpdateBusy(false);
    }
  };

  const checkForUpdate = async () => {
    setUpdateBusy(true);
    setError(null);
    try {
      const result = await api.updateCheck();
      setUpdateCheck(result);
      onNotice(
        result.available
          ? `Rigel ${result.offered} is available.`
          : `Rigel ${result.current} is current.`,
      );
    } catch (failure) {
      setUpdateCheck(null);
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setUpdateBusy(false);
    }
  };

  const installUpdate = async () => {
    setUpdateBusy(true);
    setError(null);
    try {
      onNotice(await api.updateInstall());
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      setUpdateBusy(false);
    }
  };

  const saveNotification = async (field: "notify_approval" | "notify_turn_done" | "notify_turn_failed", enabled: boolean) => {
    if (!settings) return;
    if (enabled && !(await ensurePermission())) {
      onNotice("The OS refused notification permission; the switches stay off in this window.");
      return;
    }
    const next = { ...settings, [field]: enabled };
    setSettings(next);
    try {
      setSettings(await api.saveSettings(next));
      onNotice(enabled ? "Notification enabled." : "Notification disabled.");
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
          <div className="settings-head">
            <h3>Spend</h3>
            <span className="pill cancelled">
              {spend ? `${spend.turns} turns` : "no data"}
            </span>
          </div>
          <p className="fine">
            Every recorded turn, grouped across sessions. The session database is the source
            of truth; nothing here is estimated a second time.
          </p>
          {spend ? (
            <>
              <p className="settings-total">
                {dollars(spend.total_micros)} total
              </p>
              <div className="harness-list">
                {spend.by_day.slice(0, 7).map((row) => (
                  <div className="harness-row spend-row" key={row.key}>
                    <span>{row.label}</span>
                    <span className="mono">{dollars(row.micros)}</span>
                    <span className="fine">{row.turns} turns</span>
                  </div>
                ))}
                {spend.by_session.slice(0, 5).map((row) => (
                  <div className="harness-row spend-row" key={row.key}>
                    <span>{row.label}</span>
                    <span className="mono">{dollars(row.micros)}</span>
                    <span className="fine">{row.turns} turns</span>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <p className="fine">Open a project to see its recorded spend.</p>
          )}
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Safety</h3>
            <span className={`pill ${safety ? "completed" : "cancelled"}`}>
              {safety ? `${dollars(safety.today_micros)} today` : "no data"}
            </span>
          </div>
          <p className="fine">
            Three writable agents at once by default; read-only helpers are not counted. The
            daily cap applies to every session in this project and stops every running agent
            when it is crossed. Both are recorded in the project database, not a settings file.
          </p>
          {safety ? (
            <div className="safety-form">
              <label>
                <span>Writable agents at once</span>
                <input
                  value={writableInput}
                  onChange={(event) => setWritableInput(event.target.value)}
                  inputMode="numeric"
                  aria-label="Writable agents at once"
                />
              </label>
              <label>
                <span>Daily cap, $</span>
                <input
                  value={dailyInput}
                  onChange={(event) => setDailyInput(event.target.value)}
                  inputMode="decimal"
                  placeholder="off"
                  aria-label="Daily cap in dollars"
                />
              </label>
              <div className="cap-form">
                <button
                  className="chip"
                  onClick={() => void saveSafety(false)}
                  disabled={safetyBusy}
                >
                  Save safety
                </button>
                <button
                  className="chip danger"
                  onClick={() => void saveSafety(true)}
                  disabled={safetyBusy}
                >
                  Turn the daily cap off
                </button>
              </div>
              <p className="fine mono">
                {safety.day} · {dollars(safety.today_micros)} of{" "}
                {dollarsLabel(safety.daily_cap_micros)} · {safety.writable_agents} writable
              </p>
            </div>
          ) : (
            <p className="fine">Open a project to see its safety settings.</p>
          )}
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Project rules</h3>
            <span className={`pill ${rules.length ? "completed" : "cancelled"}`}>
              {rules.length ? `${rules.length} source${rules.length === 1 ? "" : "s"}` : "none"}
            </span>
          </div>
          <p className="fine">
            Read-only, imported as data before every native turn. Memory stays the source of
            record; an <code>AGENTS.md</code> here does not outrank the current request.
          </p>
          {rules.length ? (
            <div className="harness-list">
              {rules.map((rule) => (
                <div className="harness-row" key={rule.path}>
                  <code className="harness-path">{rule.path}</code>
                  <p className="fine">
                    {rule.kind} · {Math.round(rule.bytes / 1024)} KiB
                    {rule.truncated ? " · truncated to the context cap" : ""}
                  </p>
                </div>
              ))}
            </div>
          ) : (
            <p className="fine">No AGENTS.md, CLAUDE.md or .cursor/rules file in this project.</p>
          )}
          <div className="settings-actions">
            <button
              className="chip"
              disabled={!project}
              onClick={() => {
                void api
                  .exportMemory(project ?? "")
                  .then((body) => {
                    if (!body) {
                      onNotice("No committed memory to export yet.");
                      return;
                    }
                    void navigator.clipboard?.writeText(body);
                    onNotice("Memory copied as an AGENTS.md block.");
                  })
                  .catch((failure) =>
                    onNotice(failure instanceof Error ? failure.message : "Export failed."),
                  );
              }}
            >
              Copy memory as AGENTS.md
            </button>
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Diagnostics</h3>
            <span className="pill cancelled">never sent</span>
          </div>
          <p className="fine">
            Gathers the database check, the newest turn and the local log into one redacted
            bundle under the app data folder. The same text is copied to the clipboard. Nothing
            is uploaded and no request is made to assemble it.
          </p>
          <div className="settings-actions">
            <button
              className="primary"
              onClick={() => void exportDiagnostics()}
              disabled={exporting || !project}
            >
              {exporting ? "Assembling…" : "Export diagnostics"}
            </button>
            {diagnostics ? (
              <button
                className="chip"
                onClick={() => {
                  void navigator.clipboard?.writeText(diagnostics.body);
                  onNotice("Diagnostics bundle copied.");
                }}
              >
                Copy again
              </button>
            ) : null}
          </div>
          {diagnostics ? (
            <p className="fine mono">{diagnostics.path}</p>
          ) : (
            <p className="fine">Open a project to enable the export.</p>
          )}
        </section>

        <section className="settings-section">
          <h3>Model and effort</h3>
          <ModelControls
            model={settings?.model ?? "gpt-5.6-sol-medium"}
            effort={settings?.effort ?? "medium"}
            disabled={!settings}
            onModel={(model, provider) =>
              void saveModel(model, settings?.effort ?? "medium", provider)
            }
            onEffort={(effort) => void saveModel(settings?.model ?? "gpt-5.6-sol-medium", effort)}
          />
          <p className="fine">
            Saved as the default for native turns. Picking a model also switches to the provider
            that serves it, including OpenRouter's free roster. Effort is sent separately as
            <code> reasoning_effort</code>; it is not baked into the model id. Subagents stay on
            the cheap allowlist and cannot be pointed at an expensive model.
          </p>
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Updates</h3>
            <span className={`pill ${updateStatus?.enabled ? "completed" : "cancelled"}`}>
              {updateStatus
                ? updateStatus.enabled
                  ? `v${updateStatus.current}`
                  : "off"
                : "checking"}
            </span>
          </div>
          <p className="fine">
            One signed check at launch, then only when you press Check now. The manifest comes
            from <code>github.com/OkVurb/Rigel/releases/latest</code>; the installer's SHA-256 and
            publisher signature are verified before anything runs. No telemetry and no request
            while the switch is off.
          </p>
          <label className="toggle">
            <input
              type="checkbox"
              checked={updateStatus?.enabled ?? false}
              disabled={!updateStatus || updateBusy}
              onChange={(event) => void saveUpdateSwitch("enabled", event.target.checked)}
            />
            Check GitHub for updates
          </label>
          <label className="toggle">
            <input
              type="checkbox"
              checked={updateStatus?.auto_install ?? false}
              disabled={!updateStatus?.enabled || updateBusy}
              onChange={(event) => void saveUpdateSwitch("auto_install", event.target.checked)}
            />
            Install automatically when a signed update is found
          </label>
          <div className="settings-actions">
            <button
              className="chip"
              onClick={() => void checkForUpdate()}
              disabled={!updateStatus?.enabled || updateBusy}
            >
              {updateBusy ? "Working\u2026" : "Check now"}
            </button>
            {updateCheck?.available ? (
              <button
                className="primary"
                onClick={() => void installUpdate()}
                disabled={updateBusy}
              >
                Install v{updateCheck.offered}
              </button>
            ) : null}
          </div>
          {updateCheck ? (
            <p className={`fine${updateCheck.available ? " accent" : ""}`}>
              {updateCheck.available
                ? `Rigel ${updateCheck.offered} is available${updateCheck.notes ? ` — ${updateCheck.notes}` : "."}`
                : `Rigel ${updateCheck.current} is current.`}
            </p>
          ) : null}
          {updateStatus && !updateStatus.key_present ? (
            <p className="fine">
              This build has no publisher key, so it cannot verify a release. Reinstall a release
              build; a development build can set <code>RIGEL_UPDATE_PUBKEY</code>.
            </p>
          ) : null}
          <p className="fine mono">{updateStatus?.manifest_url ?? ""}</p>
        </section>

        <section className="settings-section">
          <div className="settings-head">
            <h3>Desktop notifications</h3>
            <span className="pill cancelled">no sound</span>
          </div>
          <p className="fine">
            A turn you are already watching stays silent. These switches only affect events that
            happen while the window is in the background.
          </p>
          {(
            [
              ["notify_approval", "Approval waiting"],
              ["notify_turn_done", "Turn finished"],
              ["notify_turn_failed", "Turn failed"],
            ] as const
          ).map(([field, label]) => (
            <label className="toggle" key={field}>
              <input
                type="checkbox"
                checked={settings?.[field] ?? true}
                disabled={!settings}
                onChange={(event) => void saveNotification(field, event.target.checked)}
              />
              {label}
            </label>
          ))}
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
