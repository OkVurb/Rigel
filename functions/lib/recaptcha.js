'use strict';

/**
 * reCAPTCHA Enterprise assessment, kept separate from the HTTP handler so it can be tested
 * without the network.
 *
 * A token from the browser proves nothing by itself: Google has to be asked whether it was
 * minted for this site key for this action, and whether the score clears the bar. That call is
 * the whole point of Enterprise, so it happens server-side and the browser never sees the result.
 */

const ENDPOINT = 'https://recaptchaenterprise.googleapis.com/v1/projects';

/** Everything the assessment needs, minus the token itself. */
function projectUrl(projectId) {
  return `${ENDPOINT}/${encodeURIComponent(projectId)}/assessments`;
}

/**
 * @returns {Promise<{ok: boolean, reason?: string, status?: number, score?: number}>}
 *
 * `ok: false` carries the HTTP status the handler should answer with, so the caller never has to
 * guess between "the user is a robot" (403) and "Google is down" (502).
 */
async function assess({
  projectId,
  apiKey,
  accessToken,
  siteKey,
  token,
  expectedAction = 'contact',
  minScore = 0.5,
  fetchImpl = fetch,
  timeoutMs = 8000,
}) {
  if (!projectId) return { ok: false, status: 500, reason: 'no_project' };
  // The public site key is not a secret — it is in the page source — but Google requires it here
  // to prove the token was minted for *this* widget and not someone else's.
  if (!siteKey) return { ok: false, status: 500, reason: 'no_site_key' };
  // Two ways to authenticate: an API key, or the function's own service account. The service
  // account is better here — nothing to rotate, nothing to paste into a console — so it is the
  // default and a key is only used when one is configured.
  if (!apiKey && !accessToken) return { ok: false, status: 503, reason: 'not_configured' };
  if (!token || typeof token !== 'string' || token.length < 20) {
    return { ok: false, status: 400, reason: 'missing_token' };
  }

  const control = new AbortController();
  const timer = setTimeout(() => control.abort(), timeoutMs);
  let response;
  try {
    const url = apiKey
      ? `${projectUrl(projectId)}?key=${encodeURIComponent(apiKey)}`
      : projectUrl(projectId);
    const headers = { 'Content-Type': 'application/json' };
    if (!apiKey && accessToken) headers.Authorization = `Bearer ${accessToken}`;

    response = await fetchImpl(url, {
      method: 'POST',
      headers,
      signal: control.signal,
      body: JSON.stringify({
        event: {
          token,
          siteKey,
          expectedAction,
        },
      }),
    });
  } catch {
    return { ok: false, status: 502, reason: 'assess_unreachable' };
  } finally {
    clearTimeout(timer);
  }

  if (!response.ok) {
    // A 4xx here is our key, our permission or our quota, not the visitor. The browser is told
    // only that verification is unavailable; the log keeps Google's own words.
    let detail = '';
    try {
      const parsed = JSON.parse(await response.text());
      detail = parsed && parsed.error && parsed.error.message ? parsed.error.message : '';
    } catch {
      detail = '';
    }
    return {
      ok: false,
      status: 502,
      reason: `assess_http_${response.status}${detail ? `: ${detail.slice(0, 140)}` : ''}`,
    };
  }

  let body;
  try {
    body = await response.json();
  } catch {
    return { ok: false, status: 502, reason: 'assess_malformed' };
  }

  const properties = body.tokenProperties || {};
  if (properties.valid !== true) {
    return { ok: false, status: 403, reason: 'invalid_token' };
  }
  if (properties.action && properties.action !== expectedAction) {
    return { ok: false, status: 403, reason: 'action_mismatch' };
  }

  const score = typeof body.riskAnalysis?.score === 'number' ? body.riskAnalysis.score : null;
  if (score !== null && score < minScore) {
    return { ok: false, status: 403, reason: 'low_score', score };
  }

  return { ok: true, score: score === null ? undefined : score };
}

module.exports = { assess, projectUrl };
