/**
 * Settings, in pages.
 *
 * The window's settings are not one form: chat providers and harness keys have nothing to do
 * with the terminal's font, and the project's safety caps live in a different database from the
 * app's own preferences. So the nav is a page list — Agent, Providers, Harnesses, Safety,
 * Spending, Terminal, Skill shelf, Project rules, Notifications, Updates, Diagnostics — and each
 * page loads only what it shows, saves one field at a time, and states where that field lives
 * (settings.toml in the app data folder, the OS keychain, or the project database).
 *
 * Every control here is wired to something the core actually reads. A switch that changes
 * nothing is worse than no switch: it teaches the user not to trust the screen.
 */

import { useCallback, useEffect, useState } from "react";

import {
  api,
  when,
  type CloudPullReport,
  type CloudPushReport,
  type CloudStatus,
  type HarnessInfo,
  type IngestReport,
  type KeyStatus,
  type MailStatus,
  type MailThread,
  type MailThreadSummary,
  type OmnirouteStatus,
  type ProviderRow,
  type RefreshReport,
  type RuleSource,
  type SafetyView,
  type Settings,
  type SkillSource,
  type SourceStatus,
  type SpendReport,
  type UpdateCheck,
  type UpdateStatus,
} from "./api";
import { UI_SCALES, applyDisplay } from "./display";
import { ModelControls } from "./modelpicker";
import { ensurePermission } from "./notify";
import { applyTheme, type Theme } from "./theme";

interface Props {
  keys: KeyStatus | null;
  project: string | null;
  session: string | null;
  /** The shelf itself: the real index through Rust, the sample rows in the preview. */
  source: SkillSource;
  onKeys: (status: KeyStatus) => void;
  onNotice: (notice: string) => void;
}

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

const PAGES = [
  { id: "agent", label: "Agent", blurb: "Model, reasoning effort and the step budget" },
  { id: "providers", label: "Providers", blurb: "Chat adapters, their keys and the local router" },
  { id: "harnesses", label: "Harnesses", blurb: "External CLIs, what is installed, what they receive" },
  { id: "safety", label: "Safety", blurb: "Writable agents and the daily cap — per project" },
  { id: "spending", label: "Spending", blurb: "Recorded turns, grouped by day and session" },
  { id: "terminal", label: "Terminal", blurb: "Font, scrollback and the shell a tab opens" },
  { id: "appearance", label: "Appearance", blurb: "Dark or light, with the same amber accent" },
  { id: "shelf", label: "Skill shelf", blurb: "Sources, freshness and indexing a repository" },
  { id: "cloud", label: "Cloud", blurb: "Your Firebase project, the account and the shelf mirror" },
  { id: "mail", label: "Email", blurb: "The AgentMail inbox the agent reads and sends from" },
  { id: "rules", label: "Project rules", blurb: "AGENTS.md, CLAUDE.md and memory export" },
  { id: "notifications", label: "Notifications", blurb: "Desktop events while the window is hidden" },
  { id: "updates", label: "Updates", blurb: "The signed self-update" },
  { id: "diagnostics", label: "Diagnostics", blurb: "One redacted bundle for a bug report" },
] as const;

type PageId = (typeof PAGES)[number]["id"];

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
        <span>{present ? `${item.name} key set` : `Needs API key · ${item.id}`}</span>
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
 * One saved field: a labelled input that writes when it loses focus or the user presses Enter,
 * so a half-typed number is never stored. The value is clamped to the range the core accepts —
 * the same bounds `save_settings` enforces, so the UI and the file cannot disagree.
 */
function Field({
  label,
  hint,
  value,
  min,
  max,
  integer = true,
  unit,
  placeholder,
  disabled,
  onSave,
}: {
  label: string;
  hint?: string;
  value: number | string;
  min?: number;
  max?: number;
  integer?: boolean;
  unit?: string;
  placeholder?: string;
  disabled?: boolean;
  onSave: (next: number | string) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const commit = () => {
    if (typeof value === "number") {
      const parsed = integer ? Number.parseInt(draft, 10) : Number.parseFloat(draft);
      if (!Number.isFinite(parsed)) {
        setDraft(String(value));
        return;
      }
      const clamped = Math.min(max ?? parsed, Math.max(min ?? parsed, parsed));
      setDraft(String(clamped));
      if (clamped !== value) onSave(clamped);
      return;
    }
    const trimmed = draft.trim();
    setDraft(trimmed);
    if (trimmed !== value) onSave(trimmed);
  };

  return (
    <label className="settings-field">
      <span>{label}</span>
      <div className="settings-field-row">
        <input
          value={draft}
          aria-label={label}
          inputMode={typeof value === "number" ? "numeric" : undefined}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          disabled={disabled}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit();
            }
          }}
        />
        {unit ? <span className="fine mono">{unit}</span> : null}
      </div>
      {hint ? <span className="fine">{hint}</span> : null}
    </label>
  );
}

/** The shelf's state, in the colours the rest of the app uses for the same three words. */
function sourcePill(state: SourceStatus["state"]): string {
  if (state === "ok") return "pill ok";
  return state === "broken" ? "pill failed" : "pill cancelled";
}

function dollarsLabel(micros: number | null): string {
  return micros === null ? "no cap" : dollars(micros);
}

function dollars(micros: number): string {
  const sign = micros < 0 ? "-" : "";
  const value = Math.abs(micros);
  return `${sign}$${Math.floor(value / 1_000_000)}.${String(Math.floor((value % 1_000_000) / 100)).padStart(4, "0")}`;
}

export default function SettingsView({ keys, project, session, source, onKeys, onNotice }: Props) {
  const [page, setPage] = useState<PageId>("agent");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [server, setServer] = useState<OmnirouteStatus | null>(null);
  const [serverBusy, setServerBusy] = useState(false);
  const [harnesses, setHarnesses] = useState<HarnessInfo[] | null>(null);
  // The chat adapters come from Rust: one table, so a new provider appears here, in the CLI's
  // `keys status` and in the turn path at once.
  const [providers, setProviders] = useState<ProviderRow[]>([]);
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
  // The Cloud page: the project, the account, and what the last mirror did. Tokens never arrive
  // in this shape — only who is signed in and when the token next needs refreshing.
  const [cloud, setCloud] = useState<CloudStatus | null>(null);
  const [cloudBusy, setCloudBusy] = useState(false);
  const [cloudProject, setCloudProject] = useState("");
  const [cloudKey, setCloudKey] = useState("");
  const [cloudEmail, setCloudEmail] = useState("");
  const [cloudPassword, setCloudPassword] = useState("");
  const [cloudMode, setCloudMode] = useState<"signin" | "signup">("signin");
  const [lastPush, setLastPush] = useState<CloudPushReport | null>(null);
  const [lastPull, setLastPull] = useState<CloudPullReport | null>(null);
  // Whether this sitting has already sent a verification link, so the button can say "Resend".
  const [verificationSent, setVerificationSent] = useState(false);
  const [cloudAppCheckUrl, setCloudAppCheckUrl] = useState("");
  // The Email page: one inbox, one key in the keychain, and the newest few messages. Bodies are
  // fetched one at a time by the agent's own tool, not mirrored into this window.
  const [mail, setMail] = useState<MailStatus | null>(null);
  const [mailBusy, setMailBusy] = useState(false);
  const [mailInbox, setMailInbox] = useState("");
  const [mailKey, setMailKey] = useState("");
  const [mailThreads, setMailThreads] = useState<MailThreadSummary[] | null>(null);
  // The conversation that is open, and which one is being fetched. A thread is read in order:
  // the provider returns its messages oldest first and the page keeps that order.
  const [openThread, setOpenThread] = useState<MailThread | null>(null);
  const [openThreadId, setOpenThreadId] = useState<string | null>(null);
  const [mailTo, setMailTo] = useState("");
  const [mailSubject, setMailSubject] = useState("");
  const [mailText, setMailText] = useState("");
  const [mailError, setMailError] = useState<string | null>(null);
  // The shelf page: what the sources say, and what the last refresh or index actually did.
  const [sources, setSources] = useState<SourceStatus[] | null>(null);
  const [refreshReport, setRefreshReport] = useState<RefreshReport | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [repoInput, setRepoInput] = useState("");
  const [indexing, setIndexing] = useState(false);
  const [indexReport, setIndexReport] = useState<IngestReport | null>(null);
  const [shelfError, setShelfError] = useState<string | null>(null);

  const provider = settings?.provider ?? "omniroute";
  const providerInfo = providers.find((row) => row.id === provider) ?? null;
  const present = keys ? (keys.providers[provider] ?? false) : null;
  const token = providerInfo?.access_token === true;
  const credentialWord = token ? "token" : "key";
  const heldKeys = providers.filter((row) => keys?.providers[row.id]).length;
  const installed = harnesses?.filter((info) => info.installed).length ?? 0;
  const staleSources = sources?.filter((row) => row.state !== "ok").length ?? null;

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
      .providersList()
      .then((next) => live && setProviders(next))
      .catch(() => live && setProviders([]));
    api
      .updateStatus()
      .then((next) => live && setUpdateStatus(next))
      .catch(() => live && setUpdateStatus(null));
    api
      .cloudStatus()
      .then((next) => live && setCloud(next))
      .catch(() => live && setCloud(null));
    api
      .mailStatus()
      .then((next) => live && setMail(next))
      .catch(() => live && setMail(null));
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

  const loadSources = useCallback(async () => {
    try {
      setSources(await source.status());
    } catch (failure) {
      setSources(null);
      setShelfError(failure instanceof Error ? failure.message : String(failure));
    }
  }, [source]);

  // The shelf's own status, re-read when that page is opened rather than at launch: the launch
  // pass already ran in Rust (and wrote the rows this reads), so a stale line here is the newest
  // one. Every other page costs no query at all.
  useEffect(() => {
    if (page !== "shelf") return;
    void loadSources();
  }, [loadSources, page]);

  // The cloud account is read at mount for the nav line and again when the page opens: the
  // keychain can change under a long-lived window (a CLI sign-in, a removed entry).
  useEffect(() => {
    if (page !== "cloud") return;
    let live = true;
    api
      .cloudStatus()
      .then((next) => live && setCloud(next))
      .catch(() => live && setCloud(null));
    return () => {
      live = false;
    };
  }, [page]);

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

  /** Save one or more app preferences, keeping a single copy of the settings object. */
  const patch = async (fields: Partial<Settings>, notice?: string) => {
    if (!settings) return;
    const next = { ...settings, ...fields };
    setSettings(next);
    setError(null);
    try {
      setSettings(await api.saveSettings(next));
      if (notice) onNotice(notice);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  // The theme is the one setting applied before the save returns: waiting on disk to recolour
  // the window would make the switch feel broken, and a failed write is reported under it.
  const saveTheme = async (next: Theme) => {
    applyTheme(next);
    await patch(
      { theme: next },
      next === "light" ? "Light theme — same amber, measured contrast." : "Dark theme.",
    );
  };

  // D159: scale and density apply before the write finishes, the same reason the theme does —
  // a control that looks stuck reads as broken. A failed save is reported under the page.
  const saveDisplay = async (fields: { ui_scale?: number; compact_density?: boolean }) => {
    applyDisplay(fields.ui_scale ?? settings?.ui_scale, fields.compact_density ?? settings?.compact_density);
    await patch(
      fields,
      fields.ui_scale
        ? `Window at ${fields.ui_scale}%.`
        : fields.compact_density
          ? "Compact rows."
          : "Roomy rows.",
    );
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
      onNotice(`Agent provider set to ${providers.find((row) => row.id === id)?.name ?? id}.`);
      if (id === "omniroute") {
        setServer(await api.omnirouteStatus());
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const saveUpdateSwitch = async (field: "enabled" | "auto_install", enabled: boolean) => {
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

  /** The same pass `rigel skills refresh` runs, on demand. */
  const refreshSources = async () => {
    if (!source.refresh) {
      onNotice("This window cannot refresh sources.");
      return;
    }
    setRefreshing(true);
    setShelfError(null);
    try {
      const report = await source.refresh();
      setRefreshReport(report);
      setSources(await source.status());
      onNotice(
        report.refreshed > 0
          ? `Refreshed ${report.refreshed} of ${report.checked} source(s).`
          : `All ${report.checked} source(s) fresh — nothing was fetched.`,
      );
    } catch (failure) {
      setShelfError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setRefreshing(false);
    }
  };

  const indexRepo = async () => {
    const repo = repoInput.trim();
    if (!repo || !source.indexRepo) return;
    setIndexing(true);
    setShelfError(null);
    try {
      const report = await source.indexRepo(repo);
      setIndexReport(report);
      setSources(await source.status());
      onNotice(
        `${report.source}: ${report.found} skill(s) found, ${report.added} added, ${report.updated} updated.`,
      );
    } catch (failure) {
      setShelfError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setIndexing(false);
    }
  };

  const saveNotification = async (
    field: "notify_approval" | "notify_turn_done" | "notify_turn_failed",
    enabled: boolean,
  ) => {
    if (!settings) return;
    if (enabled && !(await ensurePermission())) {
      onNotice("The OS refused notification permission; the switches stay off in this window.");
      return;
    }
    await patch(
      { [field]: enabled } as Partial<Settings>,
      enabled ? "Notification enabled." : "Notification disabled.",
    );
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

  const saveProviderKey = async () => {
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
        `${providerInfo?.name ?? provider} ${credentialWord} stored in the OS keychain. This window will not read it back.`,
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const removeProviderKey = async () => {
    setBusy(true);
    setError(null);
    try {
      const status = await api.deleteKey(provider);
      onKeys(status);
      setConfirmRemove(false);
      onNotice(`${providerInfo?.name ?? provider} ${credentialWord} removed from the OS keychain.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  /** Save the project id and web API key; the core owns validation and the file. */
  const saveCloudProject = async () => {
    const project = cloudProject.trim();
    const key = cloudKey.trim();
    if (!project || !key) return;
    setCloudBusy(true);
    setError(null);
    try {
      const next = await api.cloudConfigure(project, key);
      setCloud(next);
      setCloudKey("");
      setCloudProject("");
      onNotice(`Firebase project ${next.project_id ?? project} saved.`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /**
   * The App Check endpoint, or `null` to turn App Check off. The endpoint is not a secret; the
   * tokens it returns never leave the core, and the status that comes back only names the URL.
   */
  const saveCloudAppCheck = async (url: string | null) => {
    setCloudBusy(true);
    setError(null);
    try {
      const next = await api.cloudAppCheck(url);
      setCloud(next);
      setCloudAppCheckUrl("");
      onNotice(
        next.app_check
          ? "App Check is on; Firebase requests carry a token from that endpoint."
          : "App Check is off; Firebase requests carry no App Check token.",
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /** Sign in, or create the account and sign in with the session Firebase returns. */
  const submitCloudAccount = async () => {
    const email = cloudEmail.trim();
    if (!email || !cloudPassword) return;
    setCloudBusy(true);
    setError(null);
    try {
      const next =
        cloudMode === "signup"
          ? await api.cloudSignup(email, cloudPassword)
          : await api.cloudLogin(email, cloudPassword);
      setCloud(next);
      setCloudPassword("");
      onNotice(
        cloudMode === "signup"
          ? `Account created for ${next.email ?? email}.`
          : `Signed in as ${next.email ?? email}.`,
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /**
   * A reset link, not an account probe. The core answers the same way for an unknown address, so
   * the copy says "if an account exists" and the two paths are indistinguishable from here.
   */
  const resetCloudPassword = async () => {
    const email = cloudEmail.trim();
    if (!email) return;
    setCloudBusy(true);
    setError(null);
    try {
      const address = await api.cloudResetPassword(email);
      onNotice(`If an account exists for ${address}, Firebase has emailed a reset link.`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /**
   * A verification link for the signed-in account. Firebase owns the link and its template; the
   * page only learns that the request was accepted, which is what turns the control into a
   * resend. Nothing about the link is stored.
   */
  const sendCloudVerification = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      const address = await api.cloudSendVerification();
      setVerificationSent(true);
      onNotice(`Firebase has emailed a verification link to ${address}.`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /**
   * Re-read the state from Firebase rather than trusting the current ID token: its claim stays
   * stale until a new token is minted, so a link clicked a minute ago would still look unverified.
   */
  const checkCloudVerification = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      const next = await api.cloudCheckVerification();
      setCloud(next);
      if (next.email_verified) setVerificationSent(false);
      onNotice(
        next.email_verified
          ? "Email verified — Firebase has confirmed this address."
          : "Not verified yet. Open the link Firebase emailed, then check again.",
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  const signOutOfCloud = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      setCloud(await api.cloudLogout());
      setLastPush(null);
      setLastPull(null);
      setVerificationSent(false);
      onNotice("Signed out of Rigel Cloud; the local refresh token is gone.");
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  const pushShelf = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      const report = await api.cloudPush();
      setLastPush(report);
      setLastPull(null);
      setCloud(await api.cloudStatus());
      const tombstones =
        report.deleted > 0
          ? ` ${report.deleted} row(s) gone locally became tombstones.`
          : "";
      const capped = report.truncated
        ? " The shelf is over the mirror's 5,000-row cap, so deletions were not propagated."
        : "";
      onNotice(`Pushed ${report.pushed} skill(s) to ${report.project_id}.${tombstones}${capped}`);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  const pullShelf = async () => {
    setCloudBusy(true);
    setError(null);
    try {
      const report = await api.cloudPull();
      setLastPull(report);
      setLastPush(null);
      setCloud(await api.cloudStatus());
      const rows =
        report.written > 0
          ? `Pulled ${report.written} new row(s) into the shelf.`
          : `Pulled ${report.received} row(s) — the shelf already had them.`;
      onNotice(
        report.deleted > 0
          ? `${rows} ${report.deleted} row(s) removed by tombstones.`
          : rows,
      );
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setCloudBusy(false);
    }
  };

  /** The right-hand column of one nav row: a short fact and, when it is a state, a dot. */
  const navMeta = (id: PageId): { text: string; dot: "idle" | "failed" | null } => {
    switch (id) {
      case "agent":
        return { text: settings?.model ? settings.model.split("/").pop() ?? "…" : "…", dot: null };
      case "providers":
        return {
          text: `${heldKeys}/${providers.length}`,
          dot: keys?.native_ready ? "idle" : "failed",
        };
      case "harnesses":
        return {
          text: `${installed} found`,
          dot: installed > 0 ? "idle" : "failed",
        };
      case "safety":
        return {
          text: safety ? `${safety.writable_agents} writable` : "…",
          dot: null,
        };
      case "spending":
        return { text: spend ? dollars(spend.total_micros) : "…", dot: null };
      case "terminal":
        return {
          text: settings ? `${settings.terminal_font_size}px` : "…",
          dot: null,
        };
      case "appearance":
        return { text: settings?.theme === "light" ? "light" : "dark", dot: null };
      case "shelf":
        return {
          text: staleSources === null ? "…" : staleSources ? `${staleSources} stale` : "fresh",
          dot: staleSources === null ? null : staleSources > 0 ? "failed" : "idle",
        };
      case "rules":
        return { text: rules.length ? `${rules.length}` : "none", dot: null };
      case "notifications": {
        const on = [
          settings?.notify_approval,
          settings?.notify_turn_done,
          settings?.notify_turn_failed,
        ].filter(Boolean).length;
        return { text: `${on}/3`, dot: on > 0 ? "idle" : "failed" };
      }
      case "cloud":
        return {
          text: cloud
            ? cloud.signed_in
              ? cloud.email ?? "signed in"
              : cloud.configured
                ? "signed out"
                : "not set"
            : "…",
          dot: cloud ? (cloud.signed_in ? "idle" : cloud.configured ? "failed" : null) : null,
        };
      case "mail":
        return {
          text: mail
            ? mail.configured && mail.key_present
              ? "ready"
              : mail.configured
                ? "no key"
                : "not set"
            : "…",
          dot: mail ? (mail.configured && mail.key_present ? "idle" : "failed") : null,
        };
      case "updates":
        return {
          text: updateStatus ? (updateStatus.enabled ? `v${updateStatus.current}` : "off") : "…",
          dot: updateStatus ? (updateStatus.enabled ? "idle" : "failed") : null,
        };
      case "diagnostics":
        return { text: diagnostics ? "ready" : "local", dot: null };
    }
  };

  const active = PAGES.find((item) => item.id === page) ?? PAGES[0];
  const theme: Theme = settings?.theme === "light" ? "light" : "dark";

  const agentPage = (
    <>
      <section className="settings-section">
        <h3>Model and effort</h3>
        <ModelControls
          model={settings?.model ?? "gpt-5.6-sol-medium"}
          effort={settings?.effort ?? "medium"}
          chatOnly={settings?.chat_models_only ?? true}
          disabled={!settings}
          onModel={(model, next) =>
            void patch(
              { model, provider: next },
              `Default model: ${model} · ${settings?.effort ?? "medium"} effort.`,
            )
          }
          onEffort={(effort) =>
            void patch({ effort }, `Reasoning effort: ${effort}.`)
          }
          onChatOnly={(chatOnly) =>
            void patch(
              { chat_models_only: chatOnly },
              chatOnly
                ? "The picker hides rows that cannot answer a chat turn."
                : "The picker shows every row, chat or not.",
            )
          }
        />
        <p className="fine">
          Saved as the default for native turns. The catalogue merges every provider that holds a
          credential — the local router, OpenRouter, Anthropic, Google's two doors — and picking a
          row also switches to the provider that serves it. Effort is sent separately as
          <code> reasoning_effort</code>; it is not baked into the model id. The chat filter's
          choice is saved too (<code>chat_models_only</code>), so the picker opens the same way
          next launch.
        </p>
        <p className="fine">
          Current line: <code>{provider}</code> · <code>{settings?.model ?? "…"}</code> ·{" "}
          <code>{settings?.effort ?? "…"}</code>
        </p>
      </section>

      <section className="settings-section">
        <h3>The turn loop</h3>
        <div className="settings-field-grid">
          <Field
            label="Steps per turn"
            value={settings?.max_steps ?? 10}
            min={1}
            max={50}
            disabled={!settings}
            hint="One step is a model reply plus its tools: read, edit, verify, answer — with room to recover from a bad call. 10 is the default; the loop stops at the bound and says so."
            onSave={(next) => void patch({ max_steps: Number(next) })}
          />
        </div>
        <p className="fine">
          Subagents stay on the cheap allowlist and keep their own, smaller bound; they cannot be
          pointed at an expensive model from here.
        </p>
      </section>
    </>
  );

  const providersPage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Chat providers</h3>
          <span className="pill cancelled">{providers.length} adapters</span>
        </div>
        <p className="fine">
          One row per adapter in the Rust table — the same table <code>rigel keys status</code>{" "}
          reads. The dotted row is what a native turn would spend.
        </p>
        <div className="harness-list">
          {providers.map((row) => {
            const stored = keys?.providers[row.id] ?? false;
            return (
              <div className="harness-row" key={row.id}>
                <div className="harness-row-head">
                  <strong>
                    {provider === row.id ? "▸ " : ""}
                    {row.name}
                  </strong>
                  <span className={`pill ${stored ? "ok" : "cancelled"}`}>
                    {stored ? (row.access_token ? "token stored" : "key stored") : "no key"}
                  </span>
                </div>
                <p className="fine">{row.hint}</p>
                <div className="settings-actions">
                  <button
                    className="chip"
                    onClick={() => void choose(row.id)}
                    disabled={!settings || provider === row.id}
                  >
                    {provider === row.id ? "In use" : "Use for new turns"}
                  </button>
                  {row.default_model ? (
                    <span className="fine mono">default {row.default_model}</span>
                  ) : null}
                </div>
              </div>
            );
          })}
        </div>
      </section>

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
            <code>/api/health</code>. Override the address with <code>RIGEL_OMNIROUTE_URL</code>.
          </p>
        </section>
      ) : null}

      <section className="settings-section">
        <div className="settings-head">
          <h3>{providerInfo?.name ?? provider}</h3>
          <span className={`pill ${present ? "completed" : "cancelled"}`}>
            {present ? `${credentialWord} stored` : `no ${credentialWord}`}
          </span>
        </div>
        <label className="key-field">
          <span>{token ? "Access token" : "API key"}</span>
          <input
            type="password"
            value={value}
            onChange={(event) => setValue(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                void saveProviderKey();
              }
            }}
            placeholder={token ? "paste an access token" : "paste the API key"}
            autoComplete="off"
            spellCheck={false}
            disabled={busy}
          />
        </label>
        <div className="settings-actions">
          <button
            className="primary"
            onClick={() => void saveProviderKey()}
            disabled={busy || !value.trim()}
          >
            {busy ? "Saving…" : "Save key"}
          </button>
          {present ? (
            confirmRemove ? (
              <>
                <button className="chip danger" onClick={() => void removeProviderKey()} disabled={busy}>
                  Confirm remove
                </button>
                <button className="chip" onClick={() => setConfirmRemove(false)} disabled={busy}>
                  Cancel
                </button>
              </>
            ) : (
              <button className="chip danger" onClick={() => setConfirmRemove(true)} disabled={busy}>
                Remove key
              </button>
            )
          ) : null}
        </div>
        <p className="fine">
          Stored as <code>rigel/{provider}</code> in the OS keychain. Headless runs can set{" "}
          <code>{providerInfo?.env ?? "its environment variable"}</code> instead; Rigel never
          writes a credential to a config file.
          {providerInfo?.key_url ? (
            <>
              {" "}
              A {credentialWord} comes from <code>{providerInfo.key_url}</code>
              {token
                ? ", or from gcloud auth print-access-token, which Rigel reads when nothing is stored here."
                : "."}
            </>
          ) : null}
        </p>
      </section>
    </>
  );

  const harnessesPage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Detected harnesses</h3>
          <button className="chip" onClick={() => void refreshHarnesses(true)} disabled={probing}>
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
          <h3>Keys a harness receives</h3>
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
              present={keys?.providers[item.id] ?? false}
              onKeys={applyKeys}
              onNotice={onNotice}
            />
          ))}
        </div>
      </section>
    </>
  );

  const safetyPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Concurrency and the daily cap</h3>
        <span className={`pill ${safety ? "completed" : "cancelled"}`}>
          {safety ? `${dollars(safety.today_micros)} today` : "no data"}
        </span>
      </div>
      <p className="fine">
        Three writable agents at once by default; read-only helpers are not counted. The daily cap
        applies to every session in this project and stops every running agent when it is crossed.
        Both are recorded in the project database, not a settings file — so a project can be
        stricter than the machine.
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
            <button className="chip" onClick={() => void saveSafety(false)} disabled={safetyBusy}>
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
            {safety.day} · {dollars(safety.today_micros)} of {dollarsLabel(safety.daily_cap_micros)}{" "}
            · {safety.writable_agents} writable
          </p>
        </div>
      ) : (
        <p className="fine">Open a project to see its safety settings.</p>
      )}
    </section>
  );

  const spendingPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Recorded spend</h3>
        <span className="pill cancelled">{spend ? `${spend.turns} turns` : "no data"}</span>
      </div>
      <p className="fine">
        Every recorded turn, grouped across sessions. The session database is the source of truth;
        nothing here is estimated a second time. A turn with no priced usage is counted but its
        dollars stay at zero rather than being guessed.
      </p>
      {spend ? (
        <>
          <p className="settings-total">{dollars(spend.total_micros)} total</p>
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
  );

  const terminalPage = (
    <>
      <section className="settings-section">
        <h3>Appearance</h3>
        <div className="settings-field-grid">
          <Field
            label="Font size"
            value={settings?.terminal_font_size ?? 13}
            min={10}
            max={20}
            unit="px"
            disabled={!settings}
            hint="Integral sizes only: a fractional advance is what makes a terminal look blurry."
            onSave={(next) => void patch({ terminal_font_size: Number(next) })}
          />
          <Field
            label="Scrollback"
            value={settings?.terminal_scrollback ?? 8_000}
            min={1_000}
            max={100_000}
            unit="lines"
            disabled={!settings}
            hint="Kept per tab in memory, not on disk."
            onSave={(next) => void patch({ terminal_scrollback: Number(next) })}
          />
        </div>
        <p className="fine">
          Both apply to an open tab as soon as they are saved; the shell itself starts the next
          time a tab opens.
        </p>
      </section>

      <section className="settings-section">
        <h3>The shell</h3>
        <div className="settings-field-grid">
          <Field
            label="Program"
            value={settings?.terminal_shell ?? ""}
            placeholder="OS default — powershell.exe on Windows, $SHELL elsewhere"
            disabled={!settings}
            hint="An absolute path, or anything on PATH. The tab's status line names what actually opened."
            onSave={(next) => void patch({ terminal_shell: String(next) })}
          />
        </div>
        <p className="fine">
          Press <strong>Restart</strong> on the Terminal page to open a new tab with a new program.
          Keys go straight to the PTY: the app does not parse or approve them.
        </p>
      </section>
    </>
  );

  const shelfPage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Freshness</h3>
          <span className={`pill ${staleSources ? "failed" : "ok"}`}>
            {staleSources === null ? "checking" : staleSources ? `${staleSources} stale` : "fresh"}
          </span>
        </div>
        <label className="toggle">
          <input
            type="checkbox"
            checked={settings?.skills_auto_update ?? true}
            disabled={!settings}
            onChange={(event) =>
              void patch(
                { skills_auto_update: event.target.checked },
                event.target.checked
                  ? "Stale sources will be re-synced at launch."
                  : "Sources now change only when you press Refresh.",
              )
            }
          />
          Refresh stale sources at launch
        </label>
        <p className="fine">
          A source whose data is older than the stale window (a day) is re-synced in the
          background when Rigel starts, so a shelf nobody has touched updates itself. The same
          pass is one command: <code>rigel skills refresh</code>. Off means the shelf changes only
          through the button below or <code>rigel skills index</code>.
        </p>
        <div className="settings-actions">
          <button className="chip" onClick={() => void refreshSources()} disabled={refreshing}>
            {refreshing ? "Refreshing…" : "Refresh stale now"}
          </button>
          {refreshReport ? (
            <span className="fine mono">
              {refreshReport.refreshed}/{refreshReport.checked} refreshed
            </span>
          ) : null}
        </div>
        {refreshReport ? (
          <div className="harness-list">
            {refreshReport.sources.map((row) => (
              <div className="harness-row" key={row.id}>
                <div className="harness-row-head">
                  <strong className="mono">{row.id}</strong>
                  <span className={`pill ${row.state === "ok" ? "ok" : "failed"}`}>{row.state}</span>
                </div>
                <p className="fine">{row.detail}</p>
              </div>
            ))}
          </div>
        ) : null}
      </section>

      <section className="settings-section">
        <h3>Sources</h3>
        <p className="fine">
          Read from the <code>sources</code> table and the clock — never from a network call, so
          this list renders offline. <code>stale</code> means the last sync worked but is old;{" "}
          <code>broken</code> keeps the time it last worked.
        </p>
        {sources ? (
          <div className="harness-list">
            {sources.map((row) => (
              <div className="harness-row" key={row.id}>
                <div className="harness-row-head">
                  <strong className="mono">{row.id}</strong>
                  <span className={sourcePill(row.state)}>{row.state}</span>
                </div>
                <p className="fine">
                  {row.base_url ? <code>{row.base_url}</code> : "built in"} · last sync{" "}
                  {when(row.last_sync_at)}
                </p>
                {row.error ? <p className="fine needs-key">{row.error}</p> : null}
              </div>
            ))}
          </div>
        ) : (
          <p className="fine">{shelfError ?? "The index is unavailable in this window."}</p>
        )}
      </section>

      <section className="settings-section">
        <h3>Index a repository</h3>
        <p className="fine">
          Walks a public GitHub repository for <code>SKILL.md</code> files, stores the bodies in
          the blob store and the searchable rows in <code>index.db</code>. The same walk{" "}
          <code>rigel skills index owner/repo</code> performs.
        </p>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>Repository</span>
            <div className="settings-field-row">
              <input
                value={repoInput}
                placeholder="owner/repo"
                aria-label="Repository to index"
                spellCheck={false}
                autoComplete="off"
                disabled={indexing || !source.indexRepo}
                onChange={(event) => setRepoInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void indexRepo();
                  }
                }}
              />
              <button
                className="chip"
                onClick={() => void indexRepo()}
                disabled={indexing || !repoInput.trim() || !source.indexRepo}
              >
                {indexing ? "Walking…" : "Index"}
              </button>
            </div>
            <span className="fine">
              {source.indexRepo
                ? `${source.label} holds the rows`
                : "This window has no real index behind it."}
            </span>
          </label>
        </div>
        {indexReport ? (
          <p className="fine mono">
            {indexReport.source} · {indexReport.repo ?? "—"}@{indexReport.branch ?? "—"} ·{" "}
            {indexReport.found} found · {indexReport.added} added · {indexReport.updated} updated ·{" "}
            {indexReport.unchanged} unchanged · {indexReport.http_requests} request(s)
            {indexReport.truncated ? " · truncated" : ""}
          </p>
        ) : null}
      </section>
    </>
  );

  const rulesPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Imported rule files</h3>
        <span className={`pill ${rules.length ? "completed" : "cancelled"}`}>
          {rules.length ? `${rules.length} source${rules.length === 1 ? "" : "s"}` : "none"}
        </span>
      </div>
      <p className="fine">
        Read-only, imported as data before every native turn. Memory stays the source of record;
        an <code>AGENTS.md</code> here does not outrank the current request.
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
  );

  const notificationsPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Desktop events</h3>
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
  );

  const updatesPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Self-update</h3>
        <span className={`pill ${updateStatus?.enabled ? "completed" : "cancelled"}`}>
          {updateStatus
            ? updateStatus.enabled
              ? `v${updateStatus.current}`
              : "off"
            : "checking"}
        </span>
      </div>
      <p className="fine">
        One signed check at launch, then only when you press Check now. The manifest comes from{" "}
        <code>github.com/OkVurb/Rigel/releases/latest</code>; the installer's SHA-256 and publisher
        signature are verified before anything runs. No telemetry and no request while the switch
        is off.
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
          <button className="primary" onClick={() => void installUpdate()} disabled={updateBusy}>
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
  );

  const diagnosticsPage = (
    <section className="settings-section">
      <div className="settings-head">
        <h3>Bug-report bundle</h3>
        <span className="pill cancelled">never sent</span>
      </div>
      <p className="fine">
        Gathers the database check, the newest turn and the local log into one redacted bundle
        under the app data folder. The same text is copied to the clipboard. Nothing is uploaded
        and no request is made to assemble it.
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
  );

  const appearancePage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Theme</h3>
          <span className="pill cancelled">{theme}</span>
        </div>
        <div className="theme-grid" role="group" aria-label="Theme">
          {(["dark", "light"] as const).map((option) => (
            <button
              key={option}
              type="button"
              className={`theme-option${theme === option ? " on" : ""}`}
              aria-pressed={theme === option}
              disabled={!settings}
              onClick={() => void saveTheme(option)}
            >
              <span className={`theme-swatch ${option}`} aria-hidden>
                <i />
                <i />
                <i />
              </span>
              <span className="theme-option-body">
                {option === "dark" ? "Dark" : "Light"}
                {theme === option ? <span className="pill ok">current</span> : null}
              </span>
            </button>
          ))}
        </div>
        <p className="fine">
          Dark is the default. Both themes keep the single amber accent: light uses a darker
          shade of the same hue for text and icons, so every label still clears AA on white. The
          terminal palette switches with the window without restarting the shell.
        </p>
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>Scale and density</h3>
          <span className="pill cancelled">{settings?.ui_scale ?? 100}%</span>
        </div>
        <div className="segmented" role="group" aria-label="Window scale">
          {UI_SCALES.map((scale) => (
            <button
              key={scale}
              type="button"
              className={settings?.ui_scale === scale ? "on" : ""}
              aria-pressed={settings?.ui_scale === scale}
              disabled={!settings}
              onClick={() => void saveDisplay({ ui_scale: scale })}
            >
              {scale}%
            </button>
          ))}
        </div>
        <div className="segmented" role="group" aria-label="Row density" style={{ marginTop: 8 }}>
          {(
            [
              ["roomy", "Roomy"],
              ["compact", "Compact"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={(settings?.compact_density ? "compact" : "roomy") === id ? "on" : ""}
              aria-pressed={(settings?.compact_density ? "compact" : "roomy") === id}
              disabled={!settings}
              onClick={() => void saveDisplay({ compact_density: id === "compact" })}
            >
              {label}
            </button>
          ))}
        </div>
        <p className="fine">
          Scale zooms the window, not the terminal: a PTY cell is measured, so its font stays the
          size the Terminal page set. Compact rows keep every line and close the gaps between
          them; the inspector folds away before the transcript is squeezed.
        </p>
      </section>
    </>
  );

  const saveMailInbox = async () => {
    const inbox = mailInbox.trim();
    if (!inbox) return;
    setMailBusy(true);
    setMailError(null);
    try {
      const next = await api.mailConfigure(inbox);
      setMail(next);
      setMailInbox("");
      onNotice(`AgentMail inbox ${next.inbox ?? inbox} saved.`);
    } catch (failure) {
      setMailError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setMailBusy(false);
    }
  };

  const saveMailKey = async () => {
    const key = mailKey.trim();
    if (!key) return;
    setMailBusy(true);
    setMailError(null);
    try {
      const next = await api.mailSetKey(key);
      setMail(next);
      setMailKey("");
      onNotice("AgentMail API key saved to the OS keychain.");
    } catch (failure) {
      setMailError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setMailBusy(false);
    }
  };

  const loadMailThreads = async () => {
    setMailBusy(true);
    setMailError(null);
    try {
      setMailThreads(await api.mailThreads(10));
    } catch (failure) {
      setMailError(failure instanceof Error ? failure.message : String(failure));
      setMailThreads(null);
    } finally {
      setMailBusy(false);
    }
  };

  /** Open one conversation. Clicking the open one again closes it. */
  const openMailThread = async (threadId: string) => {
    if (openThreadId === threadId) {
      setOpenThreadId(null);
      setOpenThread(null);
      return;
    }
    setOpenThreadId(threadId);
    setOpenThread(null);
    setMailBusy(true);
    setMailError(null);
    try {
      setOpenThread(await api.mailThread(threadId));
    } catch (failure) {
      setMailError(failure instanceof Error ? failure.message : String(failure));
      setOpenThreadId(null);
    } finally {
      setMailBusy(false);
    }
  };

  const sendMailFromPage = async () => {
    const to = mailTo.trim();
    const subject = mailSubject.trim();
    const text = mailText.trim();
    if (!to || !subject || !text) return;
    setMailBusy(true);
    setMailError(null);
    try {
      const sent = await api.mailSend(to, subject, text);
      setMailSubject("");
      setMailText("");
      onNotice(`Sent to ${to} — ${sent.message_id}`);
    } catch (failure) {
      setMailError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setMailBusy(false);
    }
  };

  const mailPage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>AgentMail inbox</h3>
          <span
            className={`pill ${mail?.configured && mail.key_present ? "ok" : "cancelled"}`}
          >
            {mail?.configured ? (mail.inbox ?? "inbox saved") : "not configured"}
          </span>
        </div>
        <p className="fine">
          The agent gets three tools over this inbox: list, read, and send. Reading needs no
          confirmation, because nothing leaves the machine. Sending is a card first — the
          recipient, subject and body — and silence is a refusal. Mail that arrives is written by
          strangers, so every body comes back labelled untrusted.
        </p>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>Inbox address</span>
            <div className="settings-field-row">
              <input
                value={mailInbox}
                aria-label="AgentMail inbox address"
                placeholder={mail?.inbox ?? "rigelagent@agentmail.to"}
                spellCheck={false}
                autoComplete="off"
                disabled={mailBusy}
                onChange={(event) => setMailInbox(event.target.value)}
              />
            </div>
          </label>
          <label className="settings-field">
            <span>API key</span>
            <div className="settings-field-row">
              <input
                type="password"
                value={mailKey}
                aria-label="AgentMail API key"
                placeholder={mail?.key_present ? "saved — paste a new one to replace" : "am_us_…"}
                autoComplete="off"
                disabled={mailBusy}
                onChange={(event) => setMailKey(event.target.value)}
              />
            </div>
          </label>
        </div>
        <div className="settings-actions">
          <button
            className="primary"
            onClick={() => void saveMailInbox()}
            disabled={mailBusy || !mailInbox.trim()}
          >
            Save inbox
          </button>
          <button
            className="chip"
            onClick={() => void saveMailKey()}
            disabled={mailBusy || !mailKey.trim()}
          >
            Save key
          </button>
          <button
            className="chip"
            onClick={() => void loadMailThreads()}
            disabled={mailBusy || !mail?.configured || !mail.key_present}
          >
            {mailBusy ? "Working…" : "Check the inbox"}
          </button>
        </div>
        {mailError ? <p className="fine">{mailError}</p> : null}
        <p className="fine mono">
          agentmail.json · the address is public; the key is in the OS keychain
        </p>
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>Conversations</h3>
          <span className="pill">
            {mailThreads ? `${mailThreads.length}` : "not fetched"}
          </span>
        </div>
        {mailThreads === null ? (
          <p className="fine">
            Check the inbox to list the conversations. Nothing is polled, and no message body is
            fetched until you open one.
          </p>
        ) : mailThreads.length === 0 ? (
          <p className="fine">The inbox is empty.</p>
        ) : (
          <ul className="fine">
            {mailThreads.map((thread) => (
              <li key={thread.thread_id}>
                <button
                  className="chip"
                  onClick={() => void openMailThread(thread.thread_id)}
                  disabled={mailBusy}
                  aria-expanded={openThreadId === thread.thread_id}
                >
                  {openThreadId === thread.thread_id ? "▾" : "▸"}{" "}
                  {thread.subject || "(no subject)"}
                </button>{" "}
                {thread.unread ? "· unread" : ""}
                <span className="mono">
                  {" "}
                  {thread.message_count} message{thread.message_count === 1 ? "" : "s"}
                  {thread.timestamp ? ` · ${thread.timestamp}` : ""}
                </span>
                {openThreadId === thread.thread_id ? (
                  <div>
                    {openThread === null ? (
                      <p className="fine">{mailBusy ? "Opening…" : "Nothing to show."}</p>
                    ) : (
                      openThread.messages.map((message, index) => (
                        <div key={message.message_id}>
                          <p className="fine">
                            <strong>{index + 1}.</strong> {message.from} ·{" "}
                            {message.timestamp ?? "—"}
                          </p>
                          <p className="fine" style={{ whiteSpace: "pre-wrap" }}>
                            {message.extracted_text ?? message.text}
                          </p>
                        </div>
                      ))
                    )}
                    <p className="fine">
                      Untrusted content: these messages were written by strangers.
                    </p>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>Send one</h3>
          <span className="pill">{mail?.inbox ?? "no inbox"}</span>
        </div>
        <p className="fine">
          Clicking Send here is the consent: the agent's own sends ask first on the approval card.
        </p>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>To</span>
            <div className="settings-field-row">
              <input
                value={mailTo}
                aria-label="Recipient address"
                placeholder="someone@example.com"
                spellCheck={false}
                autoComplete="off"
                disabled={mailBusy}
                onChange={(event) => setMailTo(event.target.value)}
              />
            </div>
          </label>
          <label className="settings-field">
            <span>Subject</span>
            <div className="settings-field-row">
              <input
                value={mailSubject}
                aria-label="Subject"
                disabled={mailBusy}
                onChange={(event) => setMailSubject(event.target.value)}
              />
            </div>
          </label>
        </div>
        <label className="settings-field">
          <span>Body</span>
          <div className="settings-field-row">
            <textarea
              value={mailText}
              aria-label="Body"
              rows={5}
              disabled={mailBusy}
              onChange={(event) => setMailText(event.target.value)}
            />
          </div>
        </label>
        <div className="settings-actions">
          <button
            className="primary"
            onClick={() => void sendMailFromPage()}
            disabled={
              mailBusy ||
              !mailTo.trim() ||
              !mailSubject.trim() ||
              !mailText.trim() ||
              !mail?.key_present
            }
          >
            Send
          </button>
        </div>
      </section>
    </>
  );

  const cloudPage = (
    <>
      <section className="settings-section">
        <div className="settings-head">
          <h3>Firebase project</h3>
          <span className={`pill ${cloud?.configured ? "ok" : "cancelled"}`}>
            {cloud?.configured ? cloud.project_id : "not configured"}
          </span>
        </div>
        <p className="fine">
          Rigel runs no server of its own: the project is yours, created in the Firebase console
          with Email/Password sign-in and Firestore. The project id and web API key are saved in{" "}
          <code>cloud.json</code>; the key is public by design, and <code>firestore.rules</code> is
          what keeps one account's shelf out of another's. <code>docs/cloud.md</code> has the
          five-minute setup.
        </p>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>Project id</span>
            <div className="settings-field-row">
              <input
                value={cloudProject}
                aria-label="Firebase project id"
                placeholder={cloud?.project_id ?? "rigel-cloud-4f2a1"}
                spellCheck={false}
                autoComplete="off"
                disabled={cloudBusy}
                onChange={(event) => setCloudProject(event.target.value)}
              />
            </div>
          </label>
          <label className="settings-field">
            <span>Web API key</span>
            <div className="settings-field-row">
              <input
                type="password"
                value={cloudKey}
                aria-label="Firebase web API key"
                placeholder={cloud?.configured ? "saved — paste a new one to replace" : "AIza…"}
                autoComplete="off"
                disabled={cloudBusy}
                onChange={(event) => setCloudKey(event.target.value)}
              />
            </div>
          </label>
        </div>
        <div className="settings-actions">
          <button
            className="primary"
            onClick={() => void saveCloudProject()}
            disabled={cloudBusy || !cloudProject.trim() || !cloudKey.trim()}
          >
            {cloud?.configured ? "Replace project" : "Save project"}
          </button>
        </div>
        <p className="fine mono">cloud.json · in the app data folder</p>
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>App Check</h3>
          <span className={`pill ${cloud?.app_check ? "ok" : "cancelled"}`}>
            {cloud?.app_check ? "on" : "off"}
          </span>
        </div>
        <p className="fine">
          App Check proves to Firebase that a request comes from a client the project owner
          approved. A desktop build has no built-in Firebase attestation, so Rigel asks an
          endpoint you run for a short-lived token and attaches it to every Firebase request. The
          endpoint answers with <code>{'{"token": "…", "expires_at": <unix seconds>}'}</code>.
        </p>
        <p className="fine">
          The endpoint is the trust boundary: a URL that mints a token for anyone is not
          attestation. Turn on enforcement for Authentication and Firestore in the Firebase
          console only after every client can attest; the iPhone sends tokens from the same kind
          of endpoint once its Cloud screen has one.
        </p>
        <div className="settings-field-grid">
          <label className="settings-field">
            <span>Attestation URL</span>
            <div className="settings-field-row">
              <input
                type="url"
                value={cloudAppCheckUrl}
                aria-label="App Check attestation URL"
                placeholder={cloud?.app_check?.attest_url ?? "https://…/app-check"}
                spellCheck={false}
                autoComplete="off"
                disabled={cloudBusy || !cloud?.configured}
                onChange={(event) => setCloudAppCheckUrl(event.target.value)}
              />
            </div>
          </label>
        </div>
        <div className="settings-actions">
          <button
            className="primary"
            onClick={() => void saveCloudAppCheck(cloudAppCheckUrl.trim())}
            disabled={cloudBusy || !cloudAppCheckUrl.trim() || !cloud?.configured}
          >
            {cloud?.app_check ? "Replace endpoint" : "Turn on App Check"}
          </button>
          {cloud?.app_check ? (
            <button
              className="chip"
              onClick={() => void saveCloudAppCheck(null)}
              disabled={cloudBusy}
            >
              Turn off
            </button>
          ) : null}
        </div>
        <p className="fine mono">cloud.json · the endpoint, never a token</p>
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>Account</h3>
          <span className={`pill ${cloud?.signed_in ? "ok" : "cancelled"}`}>
            {cloud?.signed_in ? (cloud.email ?? "signed in") : "signed out"}
          </span>
        </div>
        {cloud?.signed_in ? (
          <>
            <p className="fine">
              <code>{cloud.uid ?? "—"}</code> · the refresh token and the verification answer sit
              in the OS keychain, and this window never reads either back. The password is not
              stored anywhere.
            </p>
            <p className="fine">
              <span className={`pill ${cloud.email_verified ? "ok" : "cancelled"}`}>
                {cloud.email_verified ? "email verified" : "email not verified"}
              </span>{" "}
              {cloud.email_verified
                ? "Firebase has confirmed this address."
                : "Firebase has not confirmed this address. The shelf and the phone work either way; verification is an account property, not a gate."}
            </p>
            {!cloud.email_verified ? (
              <>
                <div className="settings-actions">
                  <button
                    className="chip"
                    onClick={() => void sendCloudVerification()}
                    disabled={cloudBusy}
                  >
                    {verificationSent ? "Resend verification email" : "Send verification email"}
                  </button>
                  <button
                    className="chip"
                    onClick={() => void checkCloudVerification()}
                    disabled={cloudBusy}
                  >
                    I&apos;ve verified — check again
                  </button>
                </div>
                {verificationSent ? (
                  <p className="fine">
                    Sent to {cloud.email}. Firebase&apos;s template is under Authentication →
                    Templates; check spam before resending.
                  </p>
                ) : null}
              </>
            ) : null}
            <div className="settings-actions">
              <button className="chip" onClick={() => void signOutOfCloud()} disabled={cloudBusy}>
                {cloudBusy ? "Working…" : "Sign out"}
              </button>
            </div>
          </>
        ) : (
          <>
            <p className="fine">
              {cloudMode === "signup"
                ? "Create an account in this Firebase project. It is signed in immediately; the password is used once and never stored."
                : "Sign in with the account that owns the shelf. The password is used for this one request and never written to disk; the refresh token is what survives a restart."}
            </p>
            <div className="settings-actions" role="group" aria-label="Account action">
              <button
                type="button"
                className={`chip${cloudMode === "signin" ? " on" : ""}`}
                aria-pressed={cloudMode === "signin"}
                disabled={cloudBusy}
                onClick={() => setCloudMode("signin")}
              >
                Sign in
              </button>
              <button
                type="button"
                className={`chip${cloudMode === "signup" ? " on" : ""}`}
                aria-pressed={cloudMode === "signup"}
                disabled={cloudBusy}
                onClick={() => setCloudMode("signup")}
              >
                Create account
              </button>
            </div>
            <div className="settings-field-grid">
              <label className="settings-field">
                <span>Email</span>
                <div className="settings-field-row">
                  <input
                    type="email"
                    value={cloudEmail}
                    aria-label="Firebase account email"
                    placeholder="you@example.com"
                    autoComplete="username"
                    disabled={cloudBusy || !cloud?.configured}
                    onChange={(event) => setCloudEmail(event.target.value)}
                  />
                </div>
              </label>
              <label className="settings-field">
                <span>Password</span>
                <div className="settings-field-row">
                  <input
                    type="password"
                    value={cloudPassword}
                    aria-label="Firebase account password"
                    autoComplete={cloudMode === "signup" ? "new-password" : "current-password"}
                    disabled={cloudBusy || !cloud?.configured}
                    onChange={(event) => setCloudPassword(event.target.value)}
                  />
                </div>
                <span className="fine">
                  {cloudMode === "signup"
                    ? "At least six characters."
                    : "Not stored; used for this one request."}
                </span>
              </label>
            </div>
            <div className="settings-actions">
              <button
                className="primary"
                onClick={() => void submitCloudAccount()}
                disabled={
                  cloudBusy ||
                  !cloud?.configured ||
                  !cloudEmail.trim() ||
                  !cloudPassword ||
                  (cloudMode === "signup" && cloudPassword.length < 6)
                }
              >
                {cloudMode === "signup"
                  ? cloudBusy
                    ? "Creating account…"
                    : "Create account"
                  : cloudBusy
                    ? "Signing in…"
                    : "Sign in"}
              </button>
              {cloudMode === "signin" ? (
                <button
                  type="button"
                  className="chip"
                  onClick={() => void resetCloudPassword()}
                  disabled={cloudBusy || !cloud?.configured || !cloudEmail.trim()}
                >
                  Send a reset link
                </button>
              ) : null}
            </div>
            {cloudMode === "signin" ? (
              <p className="fine">
                Firebase emails the reset link. The answer is the same whether or not the address
                has an account, so this page cannot be used to probe for one.
              </p>
            ) : null}
            {cloud && !cloud.configured ? (
              <p className="fine">Save a project above before signing in.</p>
            ) : null}
          </>
        )}
      </section>

      <section className="settings-section">
        <div className="settings-head">
          <h3>Shelf mirror</h3>
          <span className={`pill ${cloud?.last_sync_at ? "ok" : "cancelled"}`}>
            {cloud?.last_sync_at ? "synced" : "not synced"}
          </span>
        </div>
        <p className="fine">
          Push copies shelf metadata — names, descriptions, grades, evidence and the source URL —
          to <code>{"users/{uid}/skills"}</code>. Pull brings down rows this machine does not have.
          A row that was mirrored and is now gone locally becomes a <strong>tombstone</strong>, and
          a pull deletes the row any tombstone names. Bodies never move: a blob stays on the
          machine that fetched it, and a pulled row never overwrites a local body.
        </p>
        {cloud?.signed_in ? (
          <p className="fine">
            {cloud.last_sync_at
              ? `Last sync ${when(cloud.last_sync_at)} — push ${when(
                  cloud.last_push_at,
                )}, pull ${when(cloud.last_pull_at)}. Recorded in cloud-state.json.`
              : "No push or pull has been recorded for this account on this machine yet."}
          </p>
        ) : null}
        <div className="settings-actions">
          <button
            className="chip"
            onClick={() => void pushShelf()}
            disabled={cloudBusy || !cloud?.signed_in}
          >
            {cloudBusy ? "Working…" : "Push shelf"}
          </button>
          <button
            className="chip"
            onClick={() => void pullShelf()}
            disabled={cloudBusy || !cloud?.signed_in}
          >
            Pull shelf
          </button>
        </div>
        {lastPush ? (
          <p className="fine">
            Pushed {lastPush.pushed} skill(s) in {lastPush.batches} batch(es) to{" "}
            {lastPush.project_id}.
            {lastPush.deleted > 0
              ? ` ${lastPush.deleted} missing row(s) became tombstones.`
              : null}
            {lastPush.truncated
              ? " The shelf is over the mirror's 5,000-row cap, so deletions were not propagated."
              : null}
          </p>
        ) : null}
        {lastPull ? (
          <p className="fine">
            Pulled {lastPull.received} row(s): {lastPull.written} written, {lastPull.skipped} skipped
            over {lastPull.pages} page(s); {lastPull.tombstones} tombstone(s), {lastPull.deleted}{" "}
            removed locally.
          </p>
        ) : null}
      </section>
    </>
  );

  const body = (() => {
    switch (page) {
      case "agent":
        return agentPage;
      case "providers":
        return providersPage;
      case "harnesses":
        return harnessesPage;
      case "safety":
        return safetyPage;
      case "spending":
        return spendingPage;
      case "terminal":
        return terminalPage;
      case "appearance":
        return appearancePage;
      case "shelf":
        return shelfPage;
      case "cloud":
        return cloudPage;
      case "mail":
        return mailPage;
      case "rules":
        return rulesPage;
      case "notifications":
        return notificationsPage;
      case "updates":
        return updatesPage;
      case "diagnostics":
        return diagnosticsPage;
    }
  })();

  return (
    <>
      <aside className="list-pane settings-nav">
        <div className="list-head">
          <div className="list-title">
            <h1>Settings</h1>
            <span>{project ? "project open" : "no project"}</span>
          </div>
        </div>
        <div className="rows">
          {PAGES.map((item) => {
            const meta = navMeta(item.id);
            return (
              <button
                key={item.id}
                role="option"
                aria-selected={page === item.id}
                className={`row${page === item.id ? " on" : ""}`}
                onClick={() => setPage(item.id)}
              >
                <span className="row-text">
                  <span className="row-name">
                    {item.label}
                    <span className="nav-meta" title={meta.text}>
                      {meta.dot ? <span className={`status-dot ${meta.dot}`} /> : null}
                      {meta.text}
                    </span>
                  </span>
                  <span className="row-desc">{item.blurb}</span>
                </span>
              </button>
            );
          })}
        </div>
        <div className="roster-foot">
          <span className={`status-dot ${keys?.native_ready ? "idle" : "failed"}`} />
          <span>
            {keys?.native_ready ? "native turns ready" : "no chat provider key"}
          </span>
          <span className="grow" />
          <span className="mono">settings.toml</span>
        </div>
      </aside>

      <main id="main" tabIndex={-1} className="detail session-main settings-main">
        <header className="detail-head">
          <div>
            <p className="eyebrow">RIGEL · SETTINGS</p>
            <h2>{active.label}</h2>
            <p className="lede">{active.blurb}</p>
          </div>
        </header>

        {page === "providers" || page === "harnesses" ? (
          <p className="notice" role="status">
            {present === null || !providerInfo
              ? "Reading the keychain…"
              : present
                ? `A ${credentialWord} is stored for ${providerInfo.name}. Rigel can confirm it exists; it never reads it back into this window.`
                : `No ${credentialWord} yet for ${providerInfo.name}. Paste one below — it is never shown again.`}
          </p>
        ) : null}
        {error ? (
          <p className="notice" role="status">
            {error}
          </p>
        ) : null}

        {body}
      </main>

      <aside className="inspector">
        {page === "providers" || page === "harnesses" ? (
          <>
            <div>
              <h3>Key safety</h3>
              <p className="fine">
                Keys never enter the transcript, the session database or a config file. Saving
                clears the field; the value you typed is the only copy the window held.
              </p>
            </div>
            <div>
              <h3>Headless</h3>
              <p className="fine">
                The CLI reads the same keychain entries. <code>rigel keys set openai</code> and the
                other provider commands read from stdin and never echo the value.
              </p>
            </div>
          </>
        ) : null}
        {page === "agent" ? (
          <>
            <div>
              <h3>Where this lives</h3>
              <p className="fine">
                Model, effort and the step budget are app preferences in{" "}
                <code>settings.toml</code>. They apply to the next turn; a turn already running
                keeps the model it started with.
              </p>
            </div>
            <div>
              <h3>Why the roster is longer now</h3>
              <p className="fine">
                The picker lists every provider that holds a credential, so installing a key is
                what adds models — there is no separate catalogue to keep in sync.
              </p>
            </div>
          </>
        ) : null}
        {page === "safety" || page === "spending" ? (
          <div>
            <h3>Per project</h3>
            <p className="fine">
              Both pages read the project database: <code>safety.json</code>-style knobs are rows,
              and every dollar comes from recorded turns. A project without them falls back to the
              defaults above.
            </p>
          </div>
        ) : null}
        {page === "terminal" ? (
          <>
            <div>
              <h3>Where the shell runs</h3>
              <p className="fine">
                A real PTY opens in the project root with <code>TERM=xterm-256color</code>. Closing
                the tab kills the process; switching tabs does not.
              </p>
            </div>
            <div>
              <h3>No approval step</h3>
              <p className="fine">
                The terminal is yours: Rigel does not parse or approve what you type there, and it
                is not part of a turn's transcript.
              </p>
            </div>
          </>
        ) : null}
        {page === "appearance" ? (
          <>
            <div>
              <h3>Where this lives</h3>
              <p className="fine">
                One app preference, <code>theme</code>, in <code>settings.toml</code>. It is
                applied when the window opens and immediately on a click, so the choice survives a
                restart.
              </p>
            </div>
            <div>
              <h3>What follows it</h3>
              <p className="fine">
                Surfaces, borders, text, scrollbars and the native controls read the same tokens,
                and the terminal carries two ANSI palettes. No page is left on the old surface and
                no shell is restarted.
              </p>
            </div>
            <div>
              <h3>Measured, not flipped</h3>
              <p className="fine">
                On light, the faintest metadata is 5.26:1 on the background and 4.90:1 on raised;
                amber text is 4.71:1 there; verified is 4.84:1. The terminal's dimmest text slot
                is 5.49:1 on white.
              </p>
            </div>
          </>
        ) : null}
        {page === "shelf" ? (
          <>
            <div>
              <h3>Where the rows live</h3>
              <p className="fine">
                <code>index.db</code> holds the searchable rows and a blob store holds the bodies.
                Refreshing re-reads the source; it does not rebuild the whole index.
              </p>
            </div>
            <div>
              <h3>Refresh makes requests</h3>
              <p className="fine">
                A refresh talks to GitHub or the hub, so it needs the network. A failure leaves the
                old rows in place and marks the source <code>broken</code> with the reason.
              </p>
            </div>
          </>
        ) : null}
        {page === "rules" ? (
          <div>
            <h3>Read-only by design</h3>
            <p className="fine">
              Rigel reads these files but never writes them, and memory is never silently merged
              into a rule file. Export is a copy to the clipboard.
            </p>
          </div>
        ) : null}
        {page === "notifications" ? (
          <div>
            <h3>Nothing is sent</h3>
            <p className="fine">
              Notifications are raised by the window itself. No push service, no account and no
              request leaves the machine.
            </p>
          </div>
        ) : null}
        {page === "cloud" ? (
          <div>
            <h3>What leaves the machine</h3>
            <p className="fine">
              The sign-in request, and shelf metadata only when you press Push. Prompts,
              transcripts, project files and skill bodies are never uploaded.
            </p>
          </div>
        ) : null}
        {page === "mail" ? (
          <div>
            <h3>What leaves the machine</h3>
            <p className="fine">
              A request to AgentMail when you check the inbox, and one email when you send one —
              yours from this page, or the agent's after you approve it on the card. Nothing is
              polled, and no message body is stored in the project database.
            </p>
          </div>
        ) : null}
        {page === "updates" ? (
          <div>
            <h3>What would be verified</h3>
            <p className="fine">
              The manifest is signed; the installer must match its SHA-256 and carry the publisher
              key compiled into this build, or nothing is run.
            </p>
          </div>
        ) : null}
        {page === "diagnostics" ? (
          <div>
            <h3>Never sent</h3>
            <p className="fine">
              The bundle is written locally and copied to the clipboard. Sharing it is a deliberate
              paste, not something Rigel does for you.
            </p>
          </div>
        ) : null}
      </aside>
    </>
  );
}
