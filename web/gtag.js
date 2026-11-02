// Google tag (gtag.js) initialisation for the marketing page.
//
// It lives in a file rather than inline because the site ships a strict Content-Security-Policy
// with no 'unsafe-inline': an inline <script> would be blocked outright, and the tag would look
// installed while collecting nothing. `gtag.js` from googletagmanager.com is allowlisted in
// firebase.json instead.
//
// Only the landing page loads this. The web reader does not, and the desktop app sends nothing.
window.dataLayer = window.dataLayer || [];
function gtag() {
  dataLayer.push(arguments);
}
gtag("js", new Date());
gtag("config", "G-XCN9DVHGV7");
