import { useCallback, useEffect, useState } from "react";

import {
  api,
  when,
  type CloudPublishAllReport,
  type CloudStatus,
  type Settings,
} from "./api";
import { useMenu } from "./menu";

interface Props {
  project: string | null;
  onNotice: (text: string) => void;
  /** Jump into Settings with the right page already open. */
  onOpenSettings: (page: "cloud" | "advanced") => void;
}

function message(failure: unknown): string {
  return failure instanceof Error ? failure.message : String(failure);
}

/**
 * The account, as its own surface: who is signed in, whether Firebase verified the address, what
 * the last shelf sync did, and the one switch that decides whether finished sessions publish
 * themselves. It reads `cloud_status` — never a token — and saves preferences through the same
 * `saveSettings` the Settings pages use, so there is one copy of the truth.
 */
export default function AccountView({ project, onNotice, onOpenSettings }: Props) {
  const contextMenu = useMenu();
  const [cloud, setCloud] = useState<CloudStatus | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<CloudPublishAllReport | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const [status, prefs] = await Promise.all([api.cloudStatus(), api.getSettings()]);
      setCloud(status);
      setSettings(prefs);
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const copyText = async (text: string, label: string) => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      onNotice(`${label} copied.`);
    } catch {
      onNotice(`Could not copy ${label.toLowerCase()} from this window.`);
    }
  };

  const configured = cloud?.configured ?? false;
  const signedIn = cloud?.signed_in ?? false;
  const status = !cloud
    ? "reading"
    : signedIn
      ? "signed in"
      : configured
        ? "signed out"
        : "not configured";

  const setSync = async (enabled: boolean) => {
    if (!settings) return;
    const before = settings;
    const next = { ...settings, cloud_sync_sessions: enabled };
    setSettings(next);
    setError(null);
    try {
      setSettings(await api.saveSettings(next));
      onNotice(
        enabled
          ? "Each finished turn will publish its session; the phone sees it without a click."
          : "Automatic session publishing is off; Publish all sessions still works.",
      );
    } catch (failure) {
      setSettings(before);
      setError(message(failure));
    }
  };

  const publishAll = async () => {
    if (!project) {
      onNotice("Open a project before publishing its sessions.");
      return;
    }
    if (!signedIn) {
      onNotice("Sign in from Cloud settings before publishing sessions.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const next = await api.cloudPublishAll(project);
      setReport(next);
      onNotice(
        next.failed.length > 0
          ? `Published ${next.sessions} session(s); ${next.failed.length} failed.`
          : `Published ${next.sessions} session(s), ${next.turns} turn(s) to ${next.project_id}.`,
      );
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    if (
      !window.confirm(
        "Sign out of Rigel Cloud on this machine? Firebase keeps the account; the local refresh token is removed.",
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      setCloud(await api.cloudLogout());
      setReport(null);
      onNotice("Signed out of Rigel Cloud; the local refresh token is gone.");
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  };

  const jump = (id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const sections = [
    {
      id: "account-identity",
      label: "Identity",
      blurb: "Who is signed in, and whether Firebase verified the address",
    },
    {
      id: "account-sessions",
      label: "Session sync",
      blurb: "Publish prompts and replies to your own Firestore project",
    },
    {
      id: "account-shelf",
      label: "Shelf mirror",
      blurb: "When this machine last pushed or pulled shelf metadata",
    },
  ];

  return (
    <>
      <aside className="list-pane settings-nav">
        <div className="list-head">
          <div className="list-title">
            <h1>Account</h1>
            <span>{status}</span>
          </div>
        </div>
        <div className="rows">
          {sections.map((section) => (
            <button
              key={section.id}
              type="button"
              className="row"
              onClick={() => jump(section.id)}
              onContextMenu={(event) =>
                contextMenu.open(event, {
                  eyebrow: "Account",
                  title: section.label,
                  subtitle: section.blurb,
                  sections: [
                    {
                      label: "Section",
                      items: [
                        {
                          label: "Jump to section",
                          icon: "open",
                          onSelect: () => jump(section.id),
                        },
                        {
                          label: "Copy section name",
                          icon: "copy",
                          onSelect: () => void copyText(section.label, "Section name"),
                        },
                      ],
                    },
                  ],
                })
              }
            >
              <span className="row-text">
                <span className="row-name">{section.label}</span>
                <span className="row-desc">{section.blurb}</span>
              </span>
            </button>
          ))}
        </div>
        <div className="roster-foot">
          <span className={`status-dot ${signedIn ? "idle" : "failed"}`} />
          <span>{signedIn ? "Firebase session" : "no session"}</span>
          <span className="grow" />
          <span className="mono">{cloud?.project_id ?? "cloud.json"}</span>
        </div>
      </aside>

      <main id="main" tabIndex={-1} className="detail session-main settings-main">
        <header className="detail-head">
          <div>
            <p className="eyebrow">RIGEL · ACCOUNT</p>
            <h2>Your account</h2>
            <p className="lede">
              Identity, verification, and what this machine has sent to your own Firebase project.
            </p>
          </div>
        </header>

        {error ? (
          <p className="notice" role="status">
            {error}
          </p>
        ) : null}

        <section
          className="settings-section"
          id="account-identity"
          onContextMenu={(event) =>
            contextMenu.open(event, {
              eyebrow: "Account",
              title: cloud?.email ?? (signedIn ? "Signed in" : "No account"),
              subtitle: `${status}${cloud?.project_id ? ` · ${cloud.project_id}` : ""}`,
              sections: [
                {
                  label: "Identity",
                  items: [
                    {
                      label: "Copy email",
                      detail: cloud?.email ?? "Not signed in",
                      icon: "copy",
                      disabled: !cloud?.email,
                      onSelect: () => void copyText(cloud?.email ?? "", "Email"),
                    },
                    {
                      label: "Copy user id",
                      detail: cloud?.uid ?? "No uid stored",
                      icon: "copy",
                      disabled: !cloud?.uid,
                      onSelect: () => void copyText(cloud?.uid ?? "", "User id"),
                    },
                    {
                      label: "Copy Firebase project",
                      icon: "cloud",
                      disabled: !cloud?.project_id,
                      onSelect: () => void copyText(cloud?.project_id ?? "", "Firebase project"),
                    },
                    {
                      label: busy ? "Reading…" : "Refresh status",
                      icon: "refresh",
                      disabled: busy,
                      onSelect: () => void load(),
                    },
                  ],
                },
                {
                  label: "Account",
                  items: [
                    {
                      label: "Cloud settings",
                      detail: "Firebase project, sign-in and App Check",
                      icon: "settings",
                      onSelect: () => onOpenSettings("cloud"),
                    },
                    {
                      label: "Sign out",
                      detail: "Removes the local refresh token",
                      icon: "signout",
                      danger: true,
                      disabled: !signedIn || busy,
                      onSelect: () => void signOut(),
                    },
                  ],
                },
              ],
              footer: "The bearer token never reaches this window",
            })
          }
        >
          <div className="settings-head">
            <h3>{signedIn ? cloud?.email ?? "Signed in" : "Firebase account"}</h3>
            <span
              className={`pill ${
                signedIn ? (cloud?.email_verified ? "ok" : "cancelled") : "cancelled"
              }`}
            >
              {signedIn
                ? cloud?.email_verified
                  ? "email verified"
                  : "email not verified"
                : status}
            </span>
          </div>
          <p className="fine">
            {cloud === null
              ? "Reading cloud.json and the OS keychain…"
              : !configured
                ? "No Firebase project yet. Settings → Cloud holds the project id and web API key; the key is public by design, and Firestore rules keep one account's data out of another's."
                : !signedIn
                  ? "The project is configured; no account is signed in on this machine. The sign-in form is in Settings → Cloud."
                  : "The refresh token lives in the OS keychain. The bearer token is minted on demand and never reaches this window."}
          </p>

          <div className="settings-field-grid">
            <div className="settings-field">
              <span>Firebase project</span>
              <div className="settings-field-row">
                <code>{cloud?.project_id ?? "not configured"}</code>
              </div>
            </div>
            <div className="settings-field">
              <span>User id</span>
              <div className="settings-field-row">
                <code>{cloud?.uid ?? "—"}</code>
              </div>
            </div>
            <div className="settings-field">
              <span>Session expires</span>
              <div className="settings-field-row">
                <code>{signedIn ? when(cloud?.expires_at) : "—"}</code>
              </div>
            </div>
            <div className="settings-field">
              <span>App Check</span>
              <div className="settings-field-row">
                <code>
                  {cloud?.app_check
                    ? cloud.app_check.client_token_present
                      ? "attested · credential stored"
                      : "endpoint set · no credential"
                    : "off"}
                </code>
              </div>
            </div>
          </div>

          <div className="settings-actions">
            <button className="chip" onClick={() => void load()} disabled={busy}>
              {busy ? "Reading…" : "Refresh status"}
            </button>
            <button className="chip" onClick={() => onOpenSettings("cloud")}>
              Cloud settings
            </button>
            {signedIn ? (
              <button className="chip" onClick={() => void signOut()} disabled={busy}>
                Sign out
              </button>
            ) : null}
          </div>
        </section>

        <section className="settings-section" id="account-sessions">
          <div className="settings-head">
            <h3>Session sync</h3>
            <span
              className={`pill ${settings?.cloud_sync_sessions ? "ok" : "cancelled"}`}
            >
              {settings?.cloud_sync_sessions ? "automatic" : "manual"}
            </span>
          </div>
          <p className="fine">
            Publishing writes a session's prompts and the assistant's own replies to{" "}
            <code>{"users/{uid}/sessions"}</code>. Tool calls, command output, file changes and the
            project path stay on this machine. Publishing is an upsert: later turns rewrite the
            session, and a turn deleted locally is not deleted online.
          </p>

          <label className="toggle">
            <input
              type="checkbox"
              checked={settings?.cloud_sync_sessions ?? false}
              disabled={!settings || !signedIn || busy}
              onChange={(event) => void setSync(event.target.checked)}
            />
            publish each session after its turn finishes
          </label>
          <p className="fine">
            {signedIn
              ? "Off means only the buttons publish: Publish to phone in a session, or Publish all sessions here."
              : "Sign in from Cloud settings to enable automatic publishing."}
          </p>

          <div className="settings-actions">
            <button
              className="chip"
              onClick={() => void publishAll()}
              disabled={busy || !signedIn || !project}
              title={
                !project
                  ? "Open a project first"
                  : !signedIn
                    ? "Sign in first"
                    : "Publish every session in this project"
              }
            >
              {busy ? "Working…" : "Publish all sessions"}
            </button>
            <button className="chip" onClick={() => onOpenSettings("advanced")}>
              All preferences
            </button>
          </div>
          {!project ? (
            <p className="fine">Open a project to publish its sessions.</p>
          ) : null}
          {report ? (
            <p className="fine">
              {report.sessions} session(s), {report.turns} turn(s) on {report.project_id}.
              {report.failed.length > 0
                ? ` ${report.failed.length} failed: ${report.failed[0]}`
                : null}
            </p>
          ) : null}
        </section>

        <section className="settings-section" id="account-shelf">
          <div className="settings-head">
            <h3>Shelf mirror</h3>
            <span className={`pill ${cloud?.last_sync_at ? "ok" : "cancelled"}`}>
              {cloud?.last_sync_at ? "synced" : "not synced"}
            </span>
          </div>
          <p className="fine">
            Shelf metadata is separate from sessions: names, descriptions, grades and source URLs
            move when you press Push, and a tombstone removes a row after a local deletion. Bodies
            stay on the machine that fetched them.
          </p>
          <div className="settings-field-grid">
            <div className="settings-field">
              <span>Last push</span>
              <div className="settings-field-row">
                <code>{when(cloud?.last_push_at)}</code>
              </div>
            </div>
            <div className="settings-field">
              <span>Last pull</span>
              <div className="settings-field-row">
                <code>{when(cloud?.last_pull_at)}</code>
              </div>
            </div>
            <div className="settings-field">
              <span>Last sync</span>
              <div className="settings-field-row">
                <code>{when(cloud?.last_sync_at)}</code>
              </div>
            </div>
          </div>
          <div className="settings-actions">
            <button className="chip" onClick={() => onOpenSettings("cloud")}>
              Push or pull the shelf
            </button>
          </div>
        </section>
      </main>

      <aside className="inspector">
        <div>
          <h3>What leaves this machine</h3>
          <p className="fine">
            The sign-in request; shelf metadata when you press Push; and a session's prompts and
            assistant replies when you publish it, or after each turn once automatic publishing is
            on. Tool calls, command output, file changes, project paths and skill bodies stay here.
          </p>
        </div>
        <div>
          <h3>Tokens</h3>
          <p className="fine">
            The refresh token is stored in the OS keychain and never returned to this window. A
            bearer token is minted only when a request needs one, and App Check proves the install
            to the attestation endpoint, not the account to Firebase.
          </p>
        </div>
        <div>
          <h3>Signing out</h3>
          <p className="fine">
            Sign-out forgets the local refresh token. Firebase keeps the account, the published
            sessions and the shelf; signing in again reads the same data. Deleting the account is a
            Firebase console action, not something this page does.
          </p>
        </div>
      </aside>
    </>
  );
}
