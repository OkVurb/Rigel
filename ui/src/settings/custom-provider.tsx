/**
 * Custom OpenAI-compatible door (roadmap 28).
 *
 * A base URL, its headers and the model ids its own `/models` probe returns. The probe runs
 * before Save, so an unreachable or keyless door is not written; the whole record goes to the OS
 * keychain and the renderer only ever gets the header *names* back.
 */
import { useEffect, useState } from "react";

import { api, type CustomDoorView, type CustomHeader } from "../api";

const emptyHeader = (): CustomHeader => ({ name: "", value: "" });

export function CustomProvider({ onNotice }: { onNotice: (text: string) => void }) {
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [headers, setHeaders] = useState<CustomHeader[]>([emptyHeader()]);
  const [models, setModels] = useState<string[]>([]);
  const [saved, setSaved] = useState<CustomDoorView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .customProviderGet()
      .then((door) => {
        if (!live || !door) return;
        setSaved(door);
        setName(door.name);
        setBaseUrl(door.base_url);
        setModels(door.models);
      })
      .catch((problem: unknown) => {
        if (live) setError(problem instanceof Error ? problem.message : String(problem));
      });
    return () => {
      live = false;
    };
  }, []);

  const filled = () =>
    headers.filter((header) => header.name.trim() !== "" || header.value.trim() !== "");

  const probe = async () => {
    setBusy(true);
    setError(null);
    try {
      const listed = await api.customProviderProbe(baseUrl, filled());
      setModels(listed);
      onNotice(`Probe listed ${listed.length} model${listed.length === 1 ? "" : "s"}.`);
    } catch (problem) {
      setModels([]);
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const door = await api.customProviderSave({
        name,
        base_url: baseUrl,
        headers: filled(),
        models,
      });
      setSaved(door);
      setModels(door.models);
      setHeaders(
        door.header_names.length
          ? door.header_names.map((header) => ({ name: header, value: "" }))
          : [emptyHeader()],
      );
      onNotice(`Saved ${door.base_url} to the OS keychain.`);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="settings-section custom-provider" aria-label="Custom OpenAI-compatible door">
      <div className="settings-head">
        <h3>Custom door</h3>
        <span className={`pill ${saved ? "completed" : "cancelled"}`}>
          {saved ? "saved" : "not saved"}
        </span>
      </div>
      <p className="fine">
        Any OpenAI-compatible origin. Probe reads its model list; Save writes the base URL and
        headers to the OS keychain, never to <code>settings.toml</code>. Plain HTTP is allowed only
        on this machine.
      </p>

      <div className="settings-field-grid">
        <label className="settings-field">
          <span>Name</span>
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            aria-label="Custom door name"
            placeholder="my gateway"
          />
        </label>
        <label className="settings-field">
          <span>Base URL</span>
          <input
            value={baseUrl}
            onChange={(event) => setBaseUrl(event.target.value)}
            aria-label="Custom base URL"
            placeholder="https://gateway.example/v1"
          />
        </label>
      </div>

      <div className="custom-headers">
        {headers.map((header, index) => (
          <div className="settings-field-grid" key={index}>
            <label className="settings-field">
              <span>Header name</span>
              <input
                value={header.name}
                onChange={(event) =>
                  setHeaders((current) =>
                    current.map((pair, position) =>
                      position === index ? { ...pair, name: event.target.value } : pair,
                    ),
                  )
                }
                aria-label="Header name"
                placeholder="authorization"
              />
            </label>
            <label className="settings-field">
              <span>Header value</span>
              <input
                value={header.value}
                onChange={(event) =>
                  setHeaders((current) =>
                    current.map((pair, position) =>
                      position === index ? { ...pair, value: event.target.value } : pair,
                    ),
                  )
                }
                aria-label="Header value"
                placeholder="Bearer …"
              />
            </label>
          </div>
        ))}
      </div>

      <div className="settings-actions">
        <button
          className="chip"
          type="button"
          onClick={() => setHeaders((current) => [...current, emptyHeader()])}
        >
          Add header
        </button>
        <button
          className="primary"
          type="button"
          onClick={() => void probe()}
          disabled={busy || baseUrl.trim() === ""}
        >
          {busy ? "Probing…" : "Probe"}
        </button>
        <button
          className="chip"
          type="button"
          onClick={() => void save()}
          disabled={busy || models.length === 0}
        >
          Save door
        </button>
      </div>

      {error ? (
        <p className="notice" role="status">
          {error}
        </p>
      ) : null}

      {models.length ? (
        <div aria-label="Probed models" data-testid="custom-models">
          <p className="fine">
            {models.length} model{models.length === 1 ? "" : "s"} listed by the probe:
          </p>
          <p className="mono">{models.join(" · ")}</p>
        </div>
      ) : null}

      {saved ? (
        <p className="fine" data-testid="custom-saved">
          Saved <code>{saved.base_url}</code> with{" "}
          {saved.header_names.length
            ? saved.header_names.map((header) => <code key={header}>{header}</code>)
            : "no headers"}
          . The values are in the OS keychain.
        </p>
      ) : null}
    </section>
  );
}
