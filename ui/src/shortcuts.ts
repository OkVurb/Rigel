/**
 * Q278: rebindable window shortcuts.
 *
 * A shortcut is a small value object rather than a key string, so matching is exact and a
 * rebinding cannot silently keep an old modifier. The map is a window preference in local
 * storage, like the theme and pane widths; saving emits one event so App re-reads it without a
 * reload. A binding that only matches while no field is focused is marked by its missing
 * modifier — the caller applies that rule for `/` and any plain-key replacement.
 */

export type ShortcutId = "palette" | "focus" | "search";

export interface ShortcutBinding {
  /** Ctrl on Windows/Linux, Command on macOS. */
  mod: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

export type ShortcutMap = Record<ShortcutId, ShortcutBinding>;

export const SHORTCUTS: { id: ShortcutId; label: string; detail: string }[] = [
  {
    id: "palette",
    label: "Command palette",
    detail: "Search actions, skills, sessions, turns and patches.",
  },
  {
    id: "focus",
    label: "Focus mode",
    detail: "Hide the rail, list and inspector without changing the surface.",
  },
  {
    id: "search",
    label: "Search this surface",
    detail: "Focus the shelf or settings search.",
  },
];

export const DEFAULT_SHORTCUTS: ShortcutMap = {
  palette: { mod: true, alt: false, shift: false, key: "k" },
  focus: { mod: true, alt: false, shift: true, key: "f" },
  search: { mod: false, alt: false, shift: false, key: "/" },
};

const STORAGE_KEY = "rigel.shortcuts";
export const SHORTCUTS_EVENT = "rigel://shortcuts";

function normalizeKey(key: string): string {
  if (key === " ") return "space";
  return key.toLowerCase();
}

function isBinding(value: unknown): value is ShortcutBinding {
  if (!value || typeof value !== "object") return false;
  const row = value as Partial<ShortcutBinding>;
  return (
    typeof row.mod === "boolean" &&
    typeof row.alt === "boolean" &&
    typeof row.shift === "boolean" &&
    typeof row.key === "string" &&
    row.key.length > 0
  );
}

export function loadShortcuts(): ShortcutMap {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SHORTCUTS };
    const parsed: unknown = JSON.parse(raw);
    const map = { ...DEFAULT_SHORTCUTS };
    if (parsed && typeof parsed === "object") {
      for (const id of Object.keys(DEFAULT_SHORTCUTS) as ShortcutId[]) {
        const saved = (parsed as Record<string, unknown>)[id];
        if (isBinding(saved)) map[id] = { ...saved, key: normalizeKey(saved.key) };
      }
    }
    return map;
  } catch {
    return { ...DEFAULT_SHORTCUTS };
  }
}

export function saveShortcuts(map: ShortcutMap): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    // The defaults still work if a hardened webview refuses storage.
  }
  window.dispatchEvent(new Event(SHORTCUTS_EVENT));
}

export function setShortcut(id: ShortcutId, binding: ShortcutBinding): ShortcutMap {
  const next = { ...loadShortcuts(), [id]: { ...binding, key: normalizeKey(binding.key) } };
  saveShortcuts(next);
  return next;
}

export function resetShortcuts(): ShortcutMap {
  const next = { ...DEFAULT_SHORTCUTS };
  saveShortcuts(next);
  return next;
}

export function sameBinding(one: ShortcutBinding, other: ShortcutBinding): boolean {
  return (
    one.mod === other.mod &&
    one.alt === other.alt &&
    one.shift === other.shift &&
    one.key === other.key
  );
}

export function shortcutConflict(
  id: ShortcutId,
  binding: ShortcutBinding,
  map: ShortcutMap,
): string | null {
  for (const definition of SHORTCUTS) {
    if (definition.id === id) continue;
    if (sameBinding(map[definition.id], binding)) return definition.label;
  }
  return null;
}

function keyLabel(key: string): string {
  if (key === "space") return "Space";
  return key.length === 1 ? key.toUpperCase() : key.charAt(0).toUpperCase() + key.slice(1);
}

export function formatShortcut(binding: ShortcutBinding): string {
  const parts: string[] = [];
  if (binding.mod) parts.push("Ctrl");
  if (binding.alt) parts.push("Alt");
  if (binding.shift) parts.push("Shift");
  parts.push(keyLabel(binding.key));
  return parts.join(" ");
}

export function matchesShortcut(event: KeyboardEvent, binding: ShortcutBinding): boolean {
  return (
    (event.ctrlKey || event.metaKey) === binding.mod &&
    event.altKey === binding.alt &&
    event.shiftKey === binding.shift &&
    normalizeKey(event.key) === binding.key
  );
}

/** The binding a keydown represents, or null for a modifier-only press or Escape. */
export function eventToShortcut(event: KeyboardEvent): ShortcutBinding | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key) || event.key === "Escape") {
    return null;
  }
  return {
    mod: event.ctrlKey || event.metaKey,
    alt: event.altKey,
    shift: event.shiftKey,
    key: normalizeKey(event.key),
  };
}
