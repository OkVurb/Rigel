/**
 * The Session companions: six constellation animals perched above one quiet sponsored strip.
 *
 * Their SVGs live in `ui/public/constellations` so the window can render them offline; the web
 * deploy publishes the same files under `/constellations` for the cloud copy. The character is not
 * an ad and stays when sponsored cards are off, so it can be ignored as decoration.
 */

import { useEffect, useRef, useState } from "react";

import { api, type GravityAd } from "./api";
import { sponsorSession } from "./sponsor";

type CompanionId = "ursa" | "lepus" | "aquila" | "lynx" | "vulpes" | "lupus";

interface Companion {
  id: CompanionId;
  name: string;
  kind: string;
}

const COMPANIONS: Companion[] = [
  { id: "ursa", name: "Ursa", kind: "bear" },
  { id: "aquila", name: "Aquila", kind: "eagle" },
  { id: "lepus", name: "Lepus", kind: "hare" },
  { id: "lynx", name: "Lynx", kind: "lynx" },
  { id: "vulpes", name: "Vulpes", kind: "fox" },
  { id: "lupus", name: "Lupus", kind: "wolf" },
];

const DEFAULT_COMPANION: CompanionId = "ursa";
const COMPANION_KEY = "rigel.companion";

function companionAsset(id: CompanionId): string {
  return `/constellations/${id}.svg`;
}

function isCompanion(value: string | null): value is CompanionId {
  return COMPANIONS.some((row) => row.id === value);
}

function readCompanion(): CompanionId {
  try {
    const saved = localStorage.getItem(COMPANION_KEY);
    if (saved && isCompanion(saved)) return saved;
  } catch {
    // A blocked store still gets the default for this run.
  }
  return DEFAULT_COMPANION;
}

/** The host's ad request also carries the client user agent and OS, never a file or transcript. */
function gravityDevice() {
  return {
    ua: navigator.userAgent,
    ip: "",
    os: navigator.userAgent.includes("Windows") ? "Windows" : "",
  };
}

export default function CompanionSponsor({
  project,
  sessionId,
}: {
  project: string;
  sessionId: string;
}) {
  const [ad, setAd] = useState<GravityAd | null>(null);
  const [opening, setOpening] = useState(false);
  const [failed, setFailed] = useState(false);
  const [companionId, setCompanionId] = useState<CompanionId>(readCompanion);
  const [pickerOpen, setPickerOpen] = useState(false);
  const cardRef = useRef<HTMLElement>(null);
  const dockRef = useRef<HTMLDivElement>(null);
  const petRef = useRef<HTMLButtonElement>(null);
  const impressed = useRef<string | null>(null);

  const current = COMPANIONS.find((row) => row.id === companionId) ?? COMPANIONS[0];

  // Settings are read on every session entry, so turning sponsors off removes the strip without
  // hiding the character. The request itself is omitted when the switch is off.
  useEffect(() => {
    let live = true;
    impressed.current = null;
    setAd(null);
    setOpening(false);
    setFailed(false);
    void (async () => {
      try {
        const settings = await api.gravitySettings();
        if (!live || !settings.enabled) return;
        const ads = await api.gravityAd(
          sponsorSession(project, "session"),
          [
            {
              role: "user",
              content: "Show one sponsor for a developer working in a local-first coding session.",
            },
          ],
          gravityDevice(),
        );
        if (live) setAd(ads[0] ?? null);
      } catch {
        if (live) setAd(null);
      }
    })();
    return () => {
      live = false;
    };
  }, [project, sessionId]);

  useEffect(() => {
    setPickerOpen(false);
  }, [project, sessionId]);

  // A picker anchored to a bottom bar should close on an outside pointer, Escape, or a choice.
  useEffect(() => {
    if (!pickerOpen) return;
    const closeOnPointer = (event: PointerEvent) => {
      if (!dockRef.current?.contains(event.target as Node)) setPickerOpen(false);
    };
    const closeOnKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setPickerOpen(false);
      petRef.current?.focus();
    };
    window.addEventListener("pointerdown", closeOnPointer);
    window.addEventListener("keydown", closeOnKey);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointer);
      window.removeEventListener("keydown", closeOnKey);
    };
  }, [pickerOpen]);

  // Count a view only when at least half the strip is visible, not merely mounted above the fold.
  useEffect(() => {
    const card = cardRef.current;
    if (!card || !ad?.impUrl || impressed.current === ad.impUrl) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (document.visibilityState !== "visible") return;
        if (!entries.some((entry) => entry.isIntersecting && entry.intersectionRatio >= 0.5)) return;
        if (impressed.current === ad.impUrl) return;
        impressed.current = ad.impUrl;
        void api.gravityImpression(ad.impUrl);
        observer.disconnect();
      },
      { threshold: 0.5 },
    );
    const observeVisible = () => {
      if (document.visibilityState !== "visible" || impressed.current === ad.impUrl) return;
      observer.unobserve(card);
      observer.observe(card);
    };
    observer.observe(card);
    document.addEventListener("visibilitychange", observeVisible);
    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", observeVisible);
    };
  }, [ad?.impUrl]);

  const choose = (id: CompanionId) => {
    setCompanionId(id);
    setPickerOpen(false);
    try {
      localStorage.setItem(COMPANION_KEY, id);
    } catch {
      // The choice still holds for this run.
    }
    petRef.current?.focus();
  };

  const open = async () => {
    if (!ad?.clickUrl || opening) return;
    setOpening(true);
    setFailed(false);
    try {
      await api.gravityClick(ad.clickUrl);
    } catch {
      setFailed(true);
    } finally {
      setOpening(false);
    }
  };

  return (
    <div
      ref={dockRef}
      className={`companion-dock${ad ? "" : " bare"}`}
      role="group"
      aria-label="Companion and sponsor"
    >
      <button
        ref={petRef}
        className="companion-pet"
        type="button"
        aria-haspopup="dialog"
        aria-expanded={pickerOpen}
        aria-label={`${current.name}, the constellation ${current.kind}. Choose a companion.`}
        title={`${current.name}, the constellation ${current.kind} — choose another`}
        onClick={() => setPickerOpen((open) => !open)}
      >
        <img
          className="companion-art"
          src={companionAsset(current.id)}
          alt=""
          draggable={false}
        />
      </button>

      {pickerOpen ? (
        <div
          className="companion-picker"
          role="dialog"
          aria-label="Choose a constellation companion"
        >
          <div className="companion-picker-head">
            <p>Constellation companions</p>
            <span>{COMPANIONS.length}</span>
          </div>
          <div className="companion-options">
            {COMPANIONS.map((row) => (
              <button
                key={row.id}
                className="companion-option"
                type="button"
                aria-pressed={row.id === current.id}
                aria-label={`${row.name}, the constellation ${row.kind}`}
                onClick={() => choose(row.id)}
              >
                <img src={companionAsset(row.id)} alt="" draggable={false} />
                <span className="companion-option-text">
                  <strong>{row.name}</strong>
                  <small>{row.kind}</small>
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {ad ? (
        <section ref={cardRef} className="companion-card" aria-label="Sponsored">
          <div className="companion-card-copy">
            <p className="companion-card-label">
              <span>Advertisement</span>
              {ad.brandName ? <span className="companion-card-brand">{ad.brandName}</span> : null}
            </p>
            <h3>{ad.title || "From the wire"}</h3>
            <p className="companion-card-text">{ad.adText}</p>
            {failed ? (
              <p className="companion-card-fail" role="status">
                The sponsor's page could not be opened.
              </p>
            ) : null}
          </div>
          <button
            className="companion-cta"
            type="button"
            onClick={() => void open()}
            disabled={!ad.clickUrl || opening}
          >
            {opening ? "Opening…" : ad.cta || "Learn more"} <span aria-hidden>→</span>
          </button>
        </section>
      ) : null}
    </div>
  );
}
