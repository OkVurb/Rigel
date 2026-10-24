import { useEffect, useRef } from "react";

const TEXT_FIELD =
  'input:not([type]), input[type="text"], input[type="search"], input[type="email"], ' +
  'input[type="url"], input[type="password"], input[type="number"], input[type="tel"], ' +
  'textarea, [contenteditable="true"], [contenteditable=""]';

const CONTROL =
  'button, a[href], select, summary, label, [role="button"], [role="tab"], [role="option"], ' +
  '[role="menuitem"], [role="switch"], [role="checkbox"], input[type="checkbox"], ' +
  'input[type="radio"], input[type="range"], input[type="color"], input[type="file"], ' +
  'input[type="button"], input[type="submit"], input[type="reset"]';

/** What the pointer is over, in the only three states the dot draws. */
function kindOf(target: EventTarget | null): "" | "control" | "text" {
  if (!(target instanceof Element)) return "";
  if (target.closest(TEXT_FIELD)) return "text";
  const control = target.closest(CONTROL);
  if (control && !control.matches(":disabled") && control.getAttribute("aria-disabled") !== "true") {
    return "control";
  }
  return "";
}

/**
 * Rigel's pointer.
 *
 * The native cursor is hidden (see `.rigel-cursor` in shell.css) and this amber dot takes its
 * place, so the pointer matches the instrument rather than the OS. It writes the pointer position
 * straight to the node instead of React state, keeping a 60 Hz move off the render path.
 *
 * The dot reads the element under it: controls get a slightly wider dot and ring, and a text field
 * turns it into a thin I-beam-like bar. A coarse pointer has no cursor to replace, so touch
 * devices keep their native behaviour.
 */
export default function Cursor() {
  const pointer = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const node = pointer.current;
    if (!node) return;

    const fine = window.matchMedia("(pointer: fine)");
    if (!fine.matches) return;

    const root = document.documentElement;

    let shown = false;
    let pressed = false;
    let dragging = false;
    let startX = 0;
    let startY = 0;

    const setDragging = (next: boolean) => {
      if (dragging === next) return;
      dragging = next;
      node.dataset.dragging = String(next);
      root.classList.toggle("rigel-cursor-drag", next);
    };

    const move = (event: PointerEvent) => {
      if (event.pointerType === "touch") return;
      node.style.transform = `translate3d(${event.clientX}px, ${event.clientY}px, 0)`;
      const kind = kindOf(event.target);
      if (node.dataset.kind !== kind) node.dataset.kind = kind;

      if (pressed && event.buttons === 0) {
        // The release happened outside the window; do not leave a drag state behind.
        pressed = false;
        root.classList.remove("rigel-cursor-press");
        setDragging(false);
      } else if (
        pressed &&
        !dragging &&
        Math.hypot(event.clientX - startX, event.clientY - startY) >= 4
      ) {
        setDragging(true);
      }

      if (!shown) {
        shown = true;
        root.classList.add("rigel-cursor");
        node.dataset.visible = "true";
      }
    };

    const hide = () => {
      shown = false;
      pressed = false;
      root.classList.remove("rigel-cursor", "rigel-cursor-press");
      setDragging(false);
      node.dataset.visible = "false";
      node.dataset.kind = "";
    };

    const press = (event: PointerEvent) => {
      if (event.pointerType === "touch") return;
      pressed = true;
      startX = event.clientX;
      startY = event.clientY;
      setDragging(false);

      const target = event.target;
      const chrome =
        target instanceof Element &&
        target.closest("[data-tauri-drag-region]") !== null &&
        target.closest("button, a, input, textarea, select, [role='button']") === null;
      if (chrome) setDragging(true);

      root.classList.add("rigel-cursor-press");
    };
    const release = () => {
      pressed = false;
      root.classList.remove("rigel-cursor-press");
      setDragging(false);
    };
    const leave = (event: PointerEvent) => {
      if (event.relatedTarget === null) hide();
    };

    window.addEventListener("pointermove", move, { passive: true });
    window.addEventListener("pointerout", leave);
    window.addEventListener("pointerdown", press);
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    window.addEventListener("blur", hide);

    return () => {
      root.classList.remove("rigel-cursor", "rigel-cursor-press", "rigel-cursor-drag");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerout", leave);
      window.removeEventListener("pointerdown", press);
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
      window.removeEventListener("blur", hide);
    };
  }, []);

  return (
    <div ref={pointer} className="rigel-pointer" aria-hidden="true">
      <span className="rigel-pointer-ring" />
      <span className="rigel-pointer-dot" />
    </div>
  );
}
