/**
 * The Rigel reader — a browser for what the desktop app published.
 *
 * Design rules this file follows:
 *  - Nothing written here can change a shelf or a transcript. There is no write path at all.
 *  - Published prompts and replies are untrusted text: they only ever reach the DOM through
 *    textContent, never through innerHTML.
 *  - The URL is the state. `#/s/<id>` opens a session, so a link can be bookmarked or shared with
 *    yourself, and Back works.
 */
import { initializeApp } from "https://www.gstatic.com/firebasejs/10.14.1/firebase-app.js";
import {
  GoogleAuthProvider,
  getAuth,
  getRedirectResult,
  onAuthStateChanged,
  signInWithEmailAndPassword,
  signInWithPopup,
  signInWithRedirect,
  signOut,
} from "https://www.gstatic.com/firebasejs/10.14.1/firebase-auth.js";
import {
  collection,
  getDocs,
  getFirestore,
} from "https://www.gstatic.com/firebasejs/10.14.1/firebase-firestore.js";

const config = window.RIGEL_CONFIG ?? null;
const $ = (id) => document.getElementById(id);

const el = {
  setup: $("setup"),
  auth: $("auth"),
  signin: $("signin"),
  email: $("email"),
  password: $("password"),
  google: $("google"),
  googleLabel: $("google-label"),
  googleRedirect: $("google-redirect"),
  authError: $("auth-error"),
  app: $("app"),
  sessions: $("sessions"),
  listEmpty: $("list-empty"),
  listError: $("list-error"),
  count: $("count"),
  refresh: $("refresh"),
  search: $("search"),
  searchWrap: $("search-wrap"),
  placeholder: $("placeholder"),
  session: $("session"),
  back: $("back"),
  title: $("title"),
  meta: $("meta"),
  turns: $("turns"),
  turnsError: $("turns-error"),
  whoami: $("whoami"),
  badge: $("badge"),
  signout: $("signout"),
  account: $("account"),
  theme: $("theme"),
  themeIcon: $("theme-icon"),
  sessionRow: $("session-row"),
  turnRow: $("turn-row"),
  reader: $("reader"),
};

const state = {
  user: null,
  claims: {},
  sessions: [],
  selected: null,
  filter: "",
  turnsToken: 0,
};

/* ---------------------------------- theme ---------------------------------- */

function preferredTheme() {
  const saved = localStorage.getItem("rigel.theme");
  if (saved === "light" || saved === "dark") return saved;
  return matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  el.themeIcon.textContent = theme === "dark" ? "◐" : "◑";
  el.theme.setAttribute("aria-label", `Switch to ${theme === "dark" ? "light" : "dark"} theme`);
  localStorage.setItem("rigel.theme", theme);
}

setTheme(preferredTheme());
el.theme.addEventListener("click", () => {
  setTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark");
});

/* ---------------------------------- helpers ---------------------------------- */

function say(node, message) {
  node.textContent = message;
  node.hidden = !message;
}

function view(name) {
  for (const section of [el.setup, el.auth, el.app]) section.hidden = section !== name;
}

function milliseconds(value) {
  if (value && typeof value === "object" && typeof value.toMillis === "function") {
    return value.toMillis();
  }
  return typeof value === "number" ? value : null;
}

function date(value) {
  const ms = milliseconds(value);
  if (ms === null) return "";
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function relative(value) {
  const ms = milliseconds(value);
  if (ms === null) return "";
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  return date(value);
}

/** `spend_micros` is millionths of the account's currency; show it as money, not a raw integer. */
function money(micros) {
  const value = Number(micros);
  if (!Number.isFinite(value) || value <= 0) return null;
  return (value / 1_000_000).toLocaleString(undefined, {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: value < 1_000_000 ? 4 : 2,
  });
}

function count(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

function chip(text, extra = "") {
  const span = document.createElement("span");
  span.className = `chip ${extra}`.trim();
  span.textContent = text;
  return span;
}

/* ---------------------------------- auth ---------------------------------- */

function authMessage(error) {
  switch (error?.code) {
    case "auth/invalid-email":
      return "That address is not an email address.";
    case "auth/missing-password":
      return "Enter a password.";
    case "auth/invalid-credential":
    case "auth/wrong-password":
    case "auth/user-not-found":
      return "That email and password do not match an account in this project.";
    case "auth/too-many-requests":
      return "Too many attempts. Wait a minute and try again.";
    case "auth/network-request-failed":
      return "The network refused the request. Check the connection and try again.";
    case "auth/operation-not-allowed":
      return "This project has Email/Password sign-in turned off.";
    case "auth/account-exists-with-different-credential":
      return "That address already has an account with a different sign-in method. Use that method instead.";
    case "auth/popup-blocked":
      return "The browser blocked the sign-in window. Allow pop-ups, or use “Continue in this tab”.";
    case "auth/popup-closed-by-user":
    case "auth/cancelled-popup-request":
      return "";
    case "auth/unauthorized-domain":
      return "This domain is not authorised for Google sign-in in the Firebase console.";
    default:
      return error?.message ?? "Sign-in failed.";
  }
}

function googleState(label, busy) {
  el.google.disabled = busy;
  el.googleLabel.textContent = label;
}

function wireAuth(auth) {
  const provider = new GoogleAuthProvider();
  provider.setCustomParameters({ prompt: "select_account" });

  el.google.addEventListener("click", async () => {
    say(el.authError, "");
    googleState("Opening Google…", true);
    const started = Date.now();
    try {
      await signInWithPopup(auth, provider);
    } catch (error) {
      const early = Date.now() - started < 1500;
      const recoverable =
        error?.code === "auth/popup-blocked" ||
        error?.code === "auth/operation-not-supported-in-this-environment" ||
        ((error?.code === "auth/popup-closed-by-user" ||
          error?.code === "auth/cancelled-popup-request") &&
          early);
      if (recoverable) {
        try {
          await signInWithRedirect(auth, provider);
          return;
        } catch (redirectError) {
          say(el.authError, authMessage(redirectError));
        }
      } else {
        say(el.authError, authMessage(error));
      }
    } finally {
      googleState("Continue with Google", false);
    }
  });

  el.googleRedirect.addEventListener("click", async () => {
    say(el.authError, "");
    googleState("Redirecting…", true);
    try {
      await signInWithRedirect(auth, provider);
    } catch (error) {
      say(el.authError, authMessage(error));
      googleState("Continue with Google", false);
    }
  });

  getRedirectResult(auth).catch((error) => say(el.authError, authMessage(error)));

  el.signin.addEventListener("submit", async (event) => {
    event.preventDefault();
    say(el.authError, "");
    const submit = el.signin.querySelector("button[type=submit]");
    submit.disabled = true;
    try {
      await signInWithEmailAndPassword(auth, el.email.value.trim(), el.password.value);
      el.password.value = "";
    } catch (error) {
      say(el.authError, authMessage(error));
    } finally {
      submit.disabled = false;
    }
  });

  el.signout.addEventListener("click", () => signOut(auth));
}

/* ---------------------------------- rendering ---------------------------------- */

function renderList() {
  el.sessions.replaceChildren();
  const query = state.filter.trim().toLowerCase();
  const rows = state.sessions.filter((session) => {
    if (!query) return true;
    return [session.title, session.project, session.status, session.id]
      .filter(Boolean)
      .some((field) => String(field).toLowerCase().includes(query));
  });

  el.count.textContent = query
    ? `${rows.length} of ${state.sessions.length}`
    : count(state.sessions.length, "session", "sessions");
  el.listEmpty.hidden = rows.length > 0 || state.sessions.length === 0 ? true : true;
  el.listEmpty.hidden = rows.length > 0;
  el.listEmpty.textContent =
    state.sessions.length === 0
      ? "Nothing published yet. Publish from the desktop app with `rigel cloud publish`."
      : `No session matches “${state.filter.trim()}”.`;

  for (const session of rows) {
    const row = el.sessionRow.content.firstElementChild.cloneNode(true);
    const button = row.querySelector(".row");
    button.dataset.id = session.id;
    button.setAttribute("aria-current", String(state.selected?.id === session.id));
    button.querySelector(".row-title").textContent = session.title || session.id || "Untitled";

    const facts = [
      session.project,
      session.status,
      count(Number(session.turn_count ?? 0), "turn", "turns"),
      relative(session.published_at ?? session.created_at),
    ].filter(Boolean);
    button.querySelector(".row-meta").textContent = facts.join(" · ");
    button.addEventListener("click", () => open(session.id));
    el.sessions.append(row);
  }
}

function renderHeader(session) {
  el.title.textContent = session.title || session.id || "Session";
  el.meta.replaceChildren();
  const facts = [
    session.project && chip(session.project),
    session.status && chip(session.status, "status"),
    chip(count(Number(session.turn_count ?? 0), "turn", "turns")),
    session.agent_count ? chip(count(Number(session.agent_count), "agent", "agents")) : null,
    money(session.spend_micros) && chip(`spent ${money(session.spend_micros)}`),
    (session.published_at || session.created_at) &&
      chip(`published ${date(session.published_at ?? session.created_at)}`),
  ].filter(Boolean);
  for (const item of facts) el.meta.append(item);
}

function renderTurns(turns) {
  el.turns.replaceChildren();
  if (turns.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "This session was published without any turns.";
    el.turns.append(empty);
    return;
  }

  for (const turn of turns) {
    const row = el.turnRow.content.firstElementChild.cloneNode(true);
    row.querySelector(".seq").textContent = Number(turn.seq) || Number(turn.order) + 1;
    row.querySelector(".turn-meta").textContent = [
      turn.harness,
      turn.role,
      turn.model,
      turn.status,
      relative(turn.completed_at ?? turn.started_at),
    ]
      .filter(Boolean)
      .join(" · ");
    row.querySelector(".prompt").textContent = turn.prompt || "";
    row.querySelector(".reply").textContent = turn.reply || "";

    const copy = row.querySelector(".copy");
    copy.addEventListener("click", async () => {
      const text = [turn.prompt, turn.reply].filter(Boolean).join("\n\n");
      try {
        await navigator.clipboard.writeText(text);
        copy.textContent = "copied";
        setTimeout(() => (copy.textContent = "copy"), 1200);
      } catch {
        copy.textContent = "no access";
      }
    });
    el.turns.append(row);
  }
}

/* ---------------------------------- data ---------------------------------- */

async function loadSessions(user, { keepSelection = true } = {}) {
  say(el.listError, "");
  try {
    const snapshot = await getDocs(collection(db, "users", user.uid, "sessions"));
    state.sessions = snapshot.docs
      .map((doc) => ({ id: doc.id, ...doc.data() }))
      .sort((a, b) => (milliseconds(b.created_at) ?? 0) - (milliseconds(a.created_at) ?? 0));

    if (!keepSelection) state.selected = null;
    renderList();

    const wanted = idFromHash();
    if (wanted && state.sessions.some((session) => session.id === wanted)) {
      await open(wanted, { push: false });
    } else if (!state.selected && window.innerWidth > 820 && state.sessions.length > 0) {
      await open(state.sessions[0].id);
    }
  } catch (error) {
    say(
      el.listError,
      error?.code === "permission-denied"
        ? "The rules refused this read: this account does not own these documents."
        : `Could not read the shelf: ${error?.message ?? error}`,
    );
  }
}

async function open(id, { push = true } = {}) {
  const session = state.sessions.find((row) => row.id === id);
  if (!session) return;

  state.selected = session;
  state.filter = state.filter;
  renderList();
  renderHeader(session);
  el.placeholder.hidden = true;
  el.session.hidden = false;
  document.body.classList.add("reading");
  if (push && location.hash !== `#/s/${id}`) location.hash = `#/s/${id}`;
  el.reader.scrollTop = 0;

  const token = ++state.turnsToken;
  say(el.turnsError, "");
  el.turns.replaceChildren();
  try {
    const snapshot = await getDocs(
      collection(db, "users", state.user.uid, "sessions", id, "turns"),
    );
    if (token !== state.turnsToken) return; // a faster click won
    renderTurns(
      snapshot.docs
        .map((doc) => doc.data())
        .sort((a, b) => (Number(a.order) || 0) - (Number(b.order) || 0)),
    );
  } catch (error) {
    if (token !== state.turnsToken) return;
    say(
      el.turnsError,
      error?.code === "permission-denied"
        ? "The rules refused this read."
        : `Could not read this session: ${error?.message ?? error}`,
    );
  }
}

function idFromHash() {
  const match = /^#\/s\/(.+)$/.exec(location.hash);
  return match ? decodeURIComponent(match[1]) : null;
}

/* ---------------------------------- boot ---------------------------------- */

if (!config || !config.projectId || !config.apiKey) {
  view(el.setup);
} else {
  const app = initializeApp({
    projectId: config.projectId,
    apiKey: config.apiKey,
    authDomain: config.authDomain || `${config.projectId}.firebaseapp.com`,
  });
  const auth = getAuth(app);
  const db = getFirestore(app);

  wireAuth(auth);
  view(el.auth);

  el.back.addEventListener("click", () => {
    document.body.classList.remove("reading");
    el.session.hidden = true;
    el.placeholder.hidden = false;
    if (location.hash) history.replaceState(null, "", location.pathname);
  });

  el.search.addEventListener("input", () => {
    state.filter = el.search.value;
    renderList();
  });

  el.refresh.addEventListener("click", () => {
    if (state.user) loadSessions(state.user);
  });

  document.addEventListener("keydown", (event) => {
    if (event.key === "/" && document.activeElement !== el.search) {
      event.preventDefault();
      el.search.focus();
      el.search.select();
    }
    if (event.key === "Escape" && document.activeElement === el.search) {
      el.search.value = "";
      state.filter = "";
      renderList();
      el.search.blur();
    }
  });

  window.addEventListener("hashchange", () => {
    const id = idFromHash();
    if (id) open(id, { push: false });
  });

  onAuthStateChanged(auth, async (user) => {
    state.user = user;
    if (!user) {
      view(el.auth);
      el.account.hidden = true;
      el.searchWrap.hidden = true;
      document.body.classList.remove("reading");
      state.sessions = [];
      state.selected = null;
      el.sessions.replaceChildren();
      return;
    }

    el.whoami.textContent = user.email ?? user.uid;
    el.account.hidden = false;
    el.searchWrap.hidden = false;
    view(el.app);
    document.body.classList.remove("reading");
    el.session.hidden = true;
    el.placeholder.hidden = false;

    try {
      const token = await user.getIdTokenResult();
      state.claims = token.claims || {};
      el.badge.hidden = !(state.claims.admin || state.claims.founder);
    } catch {
      el.badge.hidden = true;
    }

    await loadSessions(user, { keepSelection: false });
  });
}
