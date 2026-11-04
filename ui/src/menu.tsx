/**
 * The one right-click menu for the whole shell.
 *
 * Native menus cannot inherit the amber palette, the hairline depth or the mono metadata the rest
 * of Rigel uses. This component gives every surface the same menu shape: a titled header, grouped
 * actions, a description line, shortcuts, submenus and full keyboard navigation. Surfaces call
 * `useMenu().open(event, spec)`; the provider owns positioning, focus and dismissal.
 *
 * The menu is portalled into `#root`, so it inherits the light/dark tokens and the window scale.
 * Its coordinates are converted back out of that zoom, which keeps the pointer and the menu
 * aligned at 90%, 110% and 125%.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { RigelMark } from "./mark";

export type MenuIcon =
  | "open"
  | "copy"
  | "link"
  | "external"
  | "pin"
  | "edit"
  | "archive"
  | "refresh"
  | "cloud"
  | "settings"
  | "terminal"
  | "chat"
  | "diff"
  | "search"
  | "command"
  | "folder"
  | "plus"
  | "check"
  | "quote"
  | "play"
  | "stop"
  | "select"
  | "clear"
  | "restart"
  | "user"
  | "signout"
  | "spark"
  | "info"
  | "trash"
  | "chevron";

/** Stroke paths on a 20×20 grid. A missing icon falls back to the small diamond. */
const ICONS: Record<MenuIcon, string> = {
  open: "M4 10h12M11 5l5 5-5 5",
  copy: "M7 6.5V4.8A1.8 1.8 0 0 1 8.8 3h6.4A1.8 1.8 0 0 1 17 4.8v6.4a1.8 1.8 0 0 1-1.8 1.8h-1.7M4.8 7h6.4A1.8 1.8 0 0 1 13 8.8v6.4a1.8 1.8 0 0 1-1.8 1.8H4.8A1.8 1.8 0 0 1 3 15.2V8.8A1.8 1.8 0 0 1 4.8 7Z",
  link: "M8.2 11.8 11.8 8.2M7.3 13.9l-1 1a2.8 2.8 0 0 1-4-4l3-3a2.8 2.8 0 0 1 4 0M12.7 6.1l1-1a2.8 2.8 0 0 1 4 4l-3 3a2.8 2.8 0 0 1-4 0",
  external: "M11 4h5v5M16 4l-7 7M9 5H5.8A1.8 1.8 0 0 0 4 6.8v7.4A1.8 1.8 0 0 0 5.8 16h7.4a1.8 1.8 0 0 0 1.8-1.8V11",
  pin: "m10 3 2 4.2 4.6.7-3.3 3.2.8 4.6-4.1-2.2-4.1 2.2.8-4.6L3.4 7.9 8 7.2 10 3Z",
  edit: "m12.8 4.2 3 3M4 16l.9-4.2L13.8 2.9a1.4 1.4 0 0 1 2 0l1.3 1.3a1.4 1.4 0 0 1 0 2l-8.9 8.9L4 16Z",
  archive: "M3 5h14v3H3V5Zm1.5 3h11v7.2A1.8 1.8 0 0 1 13.7 17H6.3a1.8 1.8 0 0 1-1.8-1.8V8ZM8 11h4",
  refresh: "M16 8a6 6 0 1 0 .3 4M16 4v4h-4",
  cloud: "M6.5 15.5h8a3 3 0 0 0 .4-6 5 5 0 0 0-9.7-1.1A3.4 3.4 0 0 0 6.5 15.5Z",
  settings: "M10 7.5a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5ZM10 3v2M10 15v2M3 10h2M15 10h2M5.1 5.1l1.4 1.4M13.5 13.5l1.4 1.4M14.9 5.1l-1.4 1.4M6.5 13.5l-1.4 1.4",
  terminal: "M3 4h14v12H3V4Zm3 4 2.5 2L6 12m4.5.5H14",
  chat: "M3 4h14v9H8l-3.5 3v-3H3V4Z",
  diff: "M6 3v14M14 3v14M3 7h6M11 13h6M3 13h3M14 7h3",
  search: "M9 15A6 6 0 1 0 9 3a6 6 0 0 0 0 12Zm4.4-1.6L17 17",
  command: "M7 3a2 2 0 1 1 0 4h6a2 2 0 1 1 0-4v4a2 2 0 1 1-4 0V3a2 2 0 1 1-4 0h4v14a2 2 0 1 1-4 0 2 2 0 0 1 4 0v-4a2 2 0 1 1-4 0 2 2 0 0 1 4 0",
  folder: "M3 5.5h5l1.5 2H17v8.7A1.8 1.8 0 0 1 15.2 18H4.8A1.8 1.8 0 0 1 3 16.2V5.5Z",
  plus: "M10 4v12M4 10h12",
  check: "m4 10 4 4 8-9",
  quote: "M4 5h5v5H6c0 2 1 3 3 4M12 5h5v5h-3c0 2 1 3 3 4",
  play: "m7 4 9 6-9 6V4Z",
  stop: "M5 5h10v10H5V5Z",
  select: "M4 4h5M4 4v5M16 4h-5M16 4v5M4 16h5M4 16v-5M16 16h-5M16 16v-5",
  clear: "M4 5h12M7 5V3h6v2M6 5l.8 12h6.4L14 5M9 8v6M11 8v6",
  restart: "M4 10a6 6 0 1 1 1.8 4.3M4 16v-4h4",
  user: "M10 10a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7ZM3.5 17a6.5 6.5 0 0 1 13 0",
  signout: "M12 4h4v12h-4M4 10h10M10 6l4 4-4 4",
  spark: "m10 2.5 1.4 5.1L16.5 9l-5.1 1.4L10 15.5 8.6 10.4 3.5 9l5.1-1.4L10 2.5ZM15.5 13l.7 2.3 2.3.7-2.3.7-.7 2.3-.7-2.3L12.5 16l2.3-.7.7-2.3Z",
  info: "M10 17a7 7 0 1 0 0-14 7 7 0 0 0 0 14ZM10 9v5M10 6.5v.1",
  trash: "M4 6h12M8 6V3h4v3M6 6l.8 11h6.4L14 6M8.5 9v5M11.5 9v5",
  chevron: "m8 5 5 5-5 5",
};

export interface MenuItem {
  /** Present only so the separator branch can be told apart from an action. */
  kind?: "item";
  label: string;
  /** One quiet line under the label. Use it to explain what the action will do. */
  detail?: string;
  icon?: MenuIcon;
  hint?: string;
  disabled?: boolean;
  danger?: boolean;
  checked?: boolean;
  onSelect?: () => void | Promise<void>;
  submenu?: MenuSection[];
}

export type MenuEntry = MenuItem | { kind: "separator" };

export interface MenuSection {
  label?: string;
  items: MenuEntry[];
}

export interface MenuSpec {
  eyebrow?: string;
  title: string;
  subtitle?: string;
  sections: MenuSection[];
  footer?: string;
}

interface MenuPointEvent {
  clientX: number;
  clientY: number;
  preventDefault: () => void;
  stopPropagation: () => void;
}

interface MenuState {
  key: number;
  x: number;
  y: number;
  spec: MenuSpec;
}

interface FlatRow {
  item: MenuItem;
  sectionIndex: number;
  itemIndex: number;
}

interface MenuController {
  open: (event: MenuPointEvent | ReactMouseEvent, spec: MenuSpec) => void;
  close: () => void;
}

const MenuContext = createContext<MenuController | null>(null);

export function useMenu(): MenuController {
  const controller = useContext(MenuContext);
  if (!controller) throw new Error("useMenu must be used inside <MenuProvider>");
  return controller;
}

export function MenuProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<MenuState | null>(null);
  const restoreRef = useRef<HTMLElement | null>(null);

  const open = useCallback((event: MenuPointEvent | ReactMouseEvent, spec: MenuSpec) => {
    event.preventDefault();
    event.stopPropagation();
    const active = document.activeElement;
    restoreRef.current = active instanceof HTMLElement ? active : null;
    setState({ key: Date.now(), x: event.clientX, y: event.clientY, spec });
  }, []);

  const close = useCallback(() => {
    setState(null);
    const target = restoreRef.current;
    // Synchronous, not deferred: an action such as Rename opens its own input, and that focus
    // must win over returning focus to the row the menu was opened from.
    if (target?.isConnected) target.focus({ preventScroll: true });
  }, []);

  const controller = useMemo(() => ({ open, close }), [open, close]);

  return (
    <MenuContext.Provider value={controller}>
      {children}
      {state ? <MenuHost key={state.key} state={state} onClose={close} /> : null}
    </MenuContext.Provider>
  );
}

function MenuHost({ state, onClose }: { state: MenuState; onClose: () => void }) {
  const menuRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Record<number, HTMLButtonElement | null>>({});
  const scale = windowScale();
  const [pos, setPos] = useState({ x: state.x / scale, y: state.y / scale });
  const [active, setActive] = useState(0);
  const [subOpen, setSubOpen] = useState<number | null>(null);
  const [subActive, setSubActive] = useState(0);
  const [subUp, setSubUp] = useState(false);
  const [flipX, setFlipX] = useState(false);

  const flat = useMemo<FlatRow[]>(
    () =>
      state.spec.sections.flatMap<FlatRow>((section, sectionIndex) =>
        section.items.flatMap<FlatRow>((entry, itemIndex) =>
          entry.kind === "separator" ? [] : [{ item: entry, sectionIndex, itemIndex }],
        ),
      ),
    [state.spec],
  );

  const openSubmenu = useCallback(
    (index: number) => {
      const anchor = itemRefs.current[index];
      const top = anchor?.offsetTop ?? 0;
      setSubUp(pos.y + top + 310 > window.innerHeight / scale);
      setSubOpen(index);
      setSubActive(0);
    },
    [pos.y, scale],
  );

  const run = useCallback(
    (item: MenuItem) => {
      if (item.disabled) return;
      onClose();
      void item.onSelect?.();
    },
    [onClose],
  );

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const bounds = { width: menu.offsetWidth, height: menu.offsetHeight };
    const vw = window.innerWidth / scale;
    const vh = window.innerHeight / scale;
    const x = Math.max(8, Math.min(state.x / scale, vw - bounds.width - 8));
    const y = Math.max(8, Math.min(state.y / scale, vh - bounds.height - 8));
    setPos({ x, y });
    setFlipX(x + bounds.width + 248 > vw);
  }, [state, scale]);

  useEffect(() => {
    menuRef.current?.focus({ preventScroll: true });
  }, []);

  useEffect(() => {
    const dismiss = () => onClose();
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    window.addEventListener("blur", dismiss);
    return () => {
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("blur", dismiss);
    };
  }, [onClose]);

  const moveMain = (delta: number) => {
    if (flat.length === 0) return;
    let next = active;
    for (let step = 0; step < flat.length; step += 1) {
      next = (next + delta + flat.length) % flat.length;
      if (!flat[next].item.disabled) break;
    }
    setActive(next);
    setSubOpen(null);
  };

  const submenu: MenuSection[] = subOpen !== null ? (flat[subOpen]?.item.submenu ?? []) : [];
  const subFlat = useMemo<FlatRow[]>(
    () =>
      submenu.flatMap<FlatRow>((section, sectionIndex) =>
        section.items.flatMap<FlatRow>((entry, itemIndex) =>
          entry.kind === "separator" ? [] : [{ item: entry, sectionIndex, itemIndex }],
        ),
      ),
    [submenu],
  );

  const moveSub = (delta: number) => {
    if (subFlat.length === 0) return;
    let next = subActive;
    for (let step = 0; step < subFlat.length; step += 1) {
      next = (next + delta + subFlat.length) % subFlat.length;
      if (!subFlat[next].item.disabled) break;
    }
    setSubActive(next);
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      if (subOpen !== null) {
        setSubOpen(null);
      } else {
        onClose();
      }
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      if (subOpen !== null) moveSub(delta);
      else moveMain(delta);
      return;
    }
    if (event.key === "ArrowRight") {
      const current = flat[active];
      if (current?.item.submenu?.length) {
        event.preventDefault();
        openSubmenu(active);
      }
      return;
    }
    if (event.key === "ArrowLeft" && subOpen !== null) {
      event.preventDefault();
      setSubOpen(null);
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (subOpen !== null) {
        const picked = subFlat[subActive]?.item;
        if (picked) run(picked);
        return;
      }
      const picked = flat[active]?.item;
      if (!picked) return;
      if (picked.submenu?.length) openSubmenu(active);
      else run(picked);
      return;
    }
    if (event.key === "Tab") onClose();
  };

  const renderItem = (item: MenuItem, index: number, inSubmenu = false) => {
    const selected = inSubmenu ? subActive === index : active === index;
    return (
      <button
        key={`item-${index}`}
        ref={(node) => {
          if (!inSubmenu) itemRefs.current[index] = node;
        }}
        type="button"
        role="menuitem"
        tabIndex={-1}
        disabled={item.disabled}
        className={`ctx-item${selected ? " on" : ""}${item.danger ? " danger" : ""}`}
        aria-haspopup={item.submenu?.length ? "menu" : undefined}
        onMouseEnter={() => {
          if (inSubmenu) setSubActive(index);
          else {
            setActive(index);
            if (item.submenu?.length) openSubmenu(index);
            else setSubOpen(null);
          }
        }}
        onClick={() => (item.submenu?.length ? openSubmenu(index) : run(item))}
      >
        <span className="ctx-icon" aria-hidden>
          {item.checked ? (
            <svg viewBox="0 0 20 20" fill="none">
              <path
                d={ICONS.check}
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          ) : (
            <svg viewBox="0 0 20 20" fill="none">
              <path
                d={ICONS[item.icon ?? "spark"]}
                stroke="currentColor"
                strokeWidth="1.35"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          )}
        </span>
        <span className="ctx-copy">
          <span className="ctx-label">{item.label}</span>
          {item.detail ? <span className="ctx-detail">{item.detail}</span> : null}
        </span>
        {item.submenu?.length ? (
          <span className="ctx-chevron" aria-hidden>
            ›
          </span>
        ) : item.hint ? (
          <kbd className="ctx-shortcut">{item.hint}</kbd>
        ) : null}
      </button>
    );
  };

  const root = document.getElementById("root") ?? document.body;
  return createPortal(
    <div className="ctx-layer" onContextMenu={(event) => event.preventDefault()}>
      <button
        type="button"
        className="ctx-scrim"
        aria-label="Close menu"
        tabIndex={-1}
        onClick={onClose}
      />
      <div
        ref={menuRef}
        className="ctx-menu"
        role="menu"
        aria-label={state.spec.title}
        tabIndex={-1}
        style={{ left: pos.x, top: pos.y }}
        onKeyDown={onKeyDown}
      >
        <div className="ctx-head">
          <div className="ctx-eyebrow">
            <span className="ctx-mark">
              <RigelMark size={13} />
            </span>
            {state.spec.eyebrow ?? "Rigel"}
          </div>
          <div className="ctx-title">{state.spec.title}</div>
          {state.spec.subtitle ? <div className="ctx-subtitle">{state.spec.subtitle}</div> : null}
        </div>
        {state.spec.sections.map((section, sectionIndex) => (
          <div className="ctx-section" key={`section-${sectionIndex}`}>
            {section.label ? <div className="ctx-section-label">{section.label}</div> : null}
            {section.items.map((entry, itemIndex) => {
              if (entry.kind === "separator") {
                return <div className="ctx-sep" key={`sep-${sectionIndex}-${itemIndex}`} />;
              }
              const index = flat.findIndex((row) => row.item === entry);
              return (
                <div className="ctx-item-wrap" key={`item-wrap-${sectionIndex}-${itemIndex}`}>
                  {renderItem(entry, index)}
                  {subOpen === index && entry.submenu?.length ? (
                    <div
                      className={`ctx-submenu${flipX ? " flip-x" : ""}${subUp ? " flip-y" : ""}`}
                      role="menu"
                      aria-label={entry.label}
                    >
                      {entry.submenu.map((subSection, subSectionIndex) => (
                        <div className="ctx-section" key={`sub-${subSectionIndex}`}>
                          {subSection.label ? (
                            <div className="ctx-section-label">{subSection.label}</div>
                          ) : null}
                          {subSection.items.map((subEntry, subItemIndex) => {
                            if (subEntry.kind === "separator") {
                              return (
                                <div
                                  className="ctx-sep"
                                  key={`sub-sep-${subSectionIndex}-${subItemIndex}`}
                                />
                              );
                            }
                            const subIndex = subFlat.findIndex((row) => row.item === subEntry);
                            return renderItem(subEntry, subIndex, true);
                          })}
                        </div>
                      ))}
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        ))}
        <div className="ctx-foot">
          <span>{state.spec.footer ?? "Actions for this surface"}</span>
          <span className="ctx-foot-keys" aria-hidden>
            <kbd>↑↓</kbd>
            <kbd>↵</kbd>
            <kbd>esc</kbd>
          </span>
        </div>
      </div>
    </div>,
    root,
  );
}

function windowScale(): number {
  const raw = Number(document.documentElement.dataset.scale ?? 100);
  return Number.isFinite(raw) && raw > 0 ? raw / 100 : 1;
}
