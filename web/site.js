// The only script on the landing page.
//
// Two jobs, both enhancements rather than requirements:
//
//   1. A lightbox for the screenshots.
//   2. Point "Get Rigel" at the MSI in whatever release GitHub currently calls latest, and notice
//      when that changes while the tab is open.
//
// Everything else that moves is CSS — scroll-driven animations for the reveals and a marquee for
// the strip — so a script failure costs the page the enlarge-on-click and a version number, and
// nothing else. The download link in the markup already points at /releases/latest, so a blocked
// fetch, a rate limit or an offline visitor still lands on a page with a working MSI.
//
// It ships as a file, not inline markup, because the site's Content-Security-Policy has no
// 'unsafe-inline' and would silently drop it.
(() => {
  const box = document.querySelector(".lightbox");
  if (!box) return;

  const image = box.querySelector("img");
  const close = box.querySelector(".lightbox-close");
  const shots = [...document.querySelectorAll(".zoom")];
  let lastFocus = null;

  function open(button) {
    const source = button.querySelector("img");
    if (!source) return;
    lastFocus = button;
    image.src = source.currentSrc || source.src;
    image.alt = source.alt || "";
    box.hidden = false;
    document.body.classList.add("locked");
    close.focus();
  }

  function shut() {
    box.hidden = true;
    image.removeAttribute("src");
    document.body.classList.remove("locked");
    if (lastFocus) lastFocus.focus();
  }

  for (const button of shots) {
    button.addEventListener("click", () => open(button));
  }

  close.addEventListener("click", shut);

  box.addEventListener("click", (event) => {
    if (event.target === box) shut();
  });

  document.addEventListener("keydown", (event) => {
    if (box.hidden) return;
    if (event.key === "Escape") {
      event.preventDefault();
      shut();
    }
    // Keep the dialog a dialog: Tab stays inside it.
    if (event.key === "Tab") {
      event.preventDefault();
      close.focus();
    }
  });
})();

// Latest release.
//
// The installers are named per version (Rigel_0.1.4_x64_en-US.msi), so the link cannot be written
// once and forgotten — every release moves it. This resolves the MSI from the release GitHub marks
// latest, keeps the version in the button, and re-checks on focus and every few minutes so a tab
// left open overnight still downloads the newest build.
(() => {
  const link = document.getElementById("download");
  if (!link) return;

  const REPO = "OkVurb/Rigel";
  const LATEST = `https://api.github.com/repos/${REPO}/releases/latest`;
  const label = document.getElementById("download-label");
  const meta = document.getElementById("release-meta");
  const shipped = (link.dataset.version || "").replace(/^v/, "");
  const CHECK_EVERY = 3 * 60 * 1000;
  const MIN_GAP = 60 * 1000;

  let lastCheck = 0;
  let shown = null;

  const size = (bytes) =>
    bytes >= 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;

  // Prefer the en-US x64 MSI; fall back to any MSI rather than guessing at an .exe.
  function pickMsi(assets) {
    const msi = (assets || []).filter((asset) => /\.msi$/i.test(asset.name));
    if (!msi.length) return null;
    return msi.find((a) => /x64/i.test(a.name) && /en-US/i.test(a.name)) || msi[0];
  }

  function apply(release) {
    const version = String(release.tag_name || "").replace(/^v/, "");
    const asset = pickMsi(release.assets);
    if (asset) {
      link.href = asset.browser_download_url;
      if (label) label.textContent = `Get Rigel ${version}`;
    }
    if (meta) {
      meta.textContent = asset
        ? `Windows 10/11 · MSI · ${size(asset.size)}`
        : "Windows 10/11 · installer on GitHub";
    }
    link.classList.toggle("is-new", Boolean(shipped) && version !== shipped);
    shown = version;
  }

  function fallback() {
    link.href = `https://github.com/${REPO}/releases/latest`;
    if (meta) meta.textContent = "Windows 10/11 · latest release on GitHub";
  }

  async function check(force) {
    if (!force && Date.now() - lastCheck < MIN_GAP) return;
    lastCheck = Date.now();

    const control = new AbortController();
    const timer = setTimeout(() => control.abort(), 8000);
    try {
      const response = await fetch(LATEST, {
        signal: control.signal,
        cache: "no-store",
        headers: { Accept: "application/vnd.github+json" },
      });
      if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
      const release = await response.json();
      if (shown && release.tag_name === `v${shown}`) return;
      apply(release);
    } catch {
      // Rate limit, offline, blocked by an extension: the markup's own link still works.
      fallback();
    } finally {
      clearTimeout(timer);
    }
  }

  check(true);
  setInterval(() => check(false), CHECK_EVERY);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") check(false);
  });
})();

/* ---------- contact form ----------
   reCAPTCHA Enterprise runs invisibly: the token is executed at submit time for the `contact`
   action and handed to the function that assesses it. Nothing is trusted on this side — the
   function decides, and this code only reports what it said. */
(function () {
  const form = document.getElementById("contact-form");
  if (!form) return;

  const SITE_KEY = "6Lc7CAwuAAAAABFNlJiGKGyMDPaKOoAeTpwaVKkr";
  const ACTION = "contact";
  const status = document.getElementById("contact-status");
  const button = form.querySelector("button[type=submit]");

  function say(text, kind) {
    status.textContent = text;
    if (kind) status.dataset.kind = kind;
    else delete status.dataset.kind;
  }

  function mint() {
    return new Promise((resolve, reject) => {
      const enterprise = window.grecaptcha && window.grecaptcha.enterprise;
      if (!enterprise) {
        reject(new Error("the verifier did not load"));
        return;
      }
      enterprise.ready(async () => {
        try {
          resolve(await enterprise.execute(SITE_KEY, { action: ACTION }));
        } catch (error) {
          reject(error);
        }
      });
    });
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const message = form.message.value.trim();
    const email = form.email.value.trim();

    if (message.length < 4) {
      say("A sentence, at least.", "bad");
      form.message.focus();
      return;
    }

    button.disabled = true;
    say("Checking…");

    try {
      const token = await mint();
      const response = await fetch("/api/contact", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ token, message, email }),
      });
      const body = await response.json().catch(() => ({}));

      if (response.ok && body.ok) {
        form.reset();
        say("Sent. Thank you.", "good");
      } else if (body.error === "rate_limited") {
        const minutes = Math.max(1, Math.ceil((body.retryAfter || 60) / 60));
        say(`That's a lot of notes — try again in ${minutes} min.`, "bad");
      } else if (body.error === "verification_failed") {
        say("The check didn't pass. Reload and try once more.", "bad");
      } else {
        say("That didn't go through. Try again in a moment.", "bad");
      }
    } catch {
      say("Couldn't run the check. Disable any blocker for this page and try again.", "bad");
    } finally {
      button.disabled = false;
    }
  });
})();

/* ---------- catalogue count ----------
   The FAQ says how many skills the catalogue holds. It reads the published meta instead of
   hard-coding a number, so the sentence cannot drift from what is actually served. */
(function () {
  const node = document.querySelector("[data-skill-count]");
  if (!node) return;
  fetch("https://rigel-25128-default-rtdb.firebaseio.com/catalog/v1/meta.json", { cache: "no-store" })
    .then((response) => (response.ok ? response.json() : null))
    .then((meta) => {
      if (meta && Number.isFinite(meta.skills)) node.textContent = meta.skills.toLocaleString("en-US");
    })
    .catch(() => {
      /* the static number stays; a blocked request is not an error worth showing */
    });
})();
