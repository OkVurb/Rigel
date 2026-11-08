function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

function button(url, label) {
  return `<p><a href="${escapeHtml(url)}" style="display:inline-block;padding:12px 20px;border-radius:8px;background:#e9a23b;color:#111;text-decoration:none;font-weight:600">${escapeHtml(label)}</a></p>`;
}

export function createMailer({ config, fetchImpl = fetch, logger = console }) {
  async function send({ to, subject, html, text }) {
    if (!config.resendApiKey) {
      if (config.production) throw new Error('RESEND_API_KEY is not configured');
      logger.info({ to, subject, preview: text }, 'mail preview (no RESEND_API_KEY)');
      return { id: 'preview' };
    }
    const response = await fetchImpl('https://api.resend.com/emails', {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${config.resendApiKey}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ from: config.mailFrom, to, subject, html, text }),
    });
    if (!response.ok) {
      const detail = await response.text().catch(() => '');
      throw new Error(`resend ${response.status}: ${detail.slice(0, 200)}`);
    }
    return response.json();
  }

  return {
    send,
    async verification(to, token) {
      const url = `${config.appUrl}/verify?token=${encodeURIComponent(token)}`;
      return send({
        to,
        subject: 'Verify your Rigel account',
        text: `Verify your email and finish setting up Rigel:\n\n${url}\n\nThis link expires in 24 hours.`,
        html: `<p>Verify your email and finish setting up Rigel.</p>${button(url, 'Verify email')}<p>This link expires in 24 hours.</p>`,
      });
    },
    async reset(to, token) {
      const url = `${config.appUrl}/reset?token=${encodeURIComponent(token)}`;
      return send({
        to,
        subject: 'Reset your Rigel password',
        text: `Set a new Rigel password:\n\n${url}\n\nThis link expires in one hour. If you did not ask for it, ignore this email.`,
        html: `<p>Set a new Rigel password.</p>${button(url, 'Choose a new password')}<p>This link expires in one hour. If you did not ask for it, ignore this email.</p>`,
      });
    },
    async passwordChanged(to) {
      return send({
        to,
        subject: 'Your Rigel password changed',
        text: 'Your password was changed and every other session was signed out. If this was not you, reset your password immediately.',
        html: '<p>Your password was changed and every other session was signed out.</p><p>If this was not you, reset your password immediately.</p>',
      });
    },
    async contact(message, email) {
      return send({
        to: config.contactTo,
        subject: 'New askrigel.com contact message',
        text: `${message}\n\nFrom: ${email || 'not provided'}`,
        html: `<p>${escapeHtml(message).replaceAll('\n', '<br>')}</p><p>From: ${escapeHtml(email || 'not provided')}</p>`,
      });
    },
  };
}
