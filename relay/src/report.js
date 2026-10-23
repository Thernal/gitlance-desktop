// What the relay does with a report before GitHub sees it: checks it, takes out what is private
// again (the app did already; a report from anything else may not have), and writes the issue.

export const LIMITS = {
  title: 200,
  body: 60_000,
  // A PNG of a window, as base64.
  picture: 4_000_000,
  // Reports per installation, per address, and in all, in a day.
  perInstall: 10,
  perAddress: 30,
};

/** The report as sent, checked: `{ ok, report }` or `{ ok: false, error }`. */
export function check(payload) {
  if (!payload || typeof payload !== 'object') return { ok: false, error: 'Not a report' };
  const text = (v) => (typeof v === 'string' ? v : '');
  const report = {
    install: text(payload.install).replace(/[^A-Za-z0-9_-]/g, '').slice(0, 40),
    version: text(payload.version).replace(/[^0-9A-Za-z.+-]/g, '').slice(0, 30),
    commit: text(payload.commit).replace(/[^0-9a-z]/g, '').slice(0, 12),
    title: text(payload.title).replace(/\s+/g, ' ').trim(),
    body: text(payload.body).trim(),
    fingerprint: text(payload.fingerprint).replace(/[^0-9a-f]/g, '').slice(0, 40),
    agent: payload.agent === true,
    picture: typeof payload.picture === 'string' ? payload.picture : null,
  };
  if (report.install.length < 8) return { ok: false, error: 'No installation id' };
  if (!report.title) return { ok: false, error: 'No title' };
  if (report.title.length > LIMITS.title) return { ok: false, error: 'The title is too long' };
  if (report.body.length > LIMITS.body) return { ok: false, error: 'The report is too long' };
  if (report.picture !== null) {
    if (report.picture.length > LIMITS.picture) return { ok: false, error: 'The picture is too large' };
    if (!/^[A-Za-z0-9+/=]+$/.test(report.picture) || !report.picture.startsWith('iVBOR')) {
      return { ok: false, error: 'The picture is not a PNG' };
    }
  }
  return { ok: true, report };
}

/** Private things a report must not carry, named for what they are. */
export function scrub(text) {
  return text
    .replace(/\/(?:Users|home)\/[^/\s]+/g, '~')
    .replace(/[A-Za-z]:\\Users\\[^\\\s]+/g, '~')
    .replace(/glpat-[A-Za-z0-9_-]{10,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}/g, '<token>')
    .replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, '<email>')
    // A mention would notify someone: it is written but goes nowhere.
    .replace(/(^|[^\w`])@(?=[A-Za-z0-9-])/g, '$1@​');
}

/** A short id the reporter sees: R-7F3A2C. */
export function reportId(random = crypto.getRandomValues(new Uint8Array(3))) {
  return 'R-' + Array.from(random, (b) => b.toString(16).padStart(2, '0')).join('').toUpperCase();
}

/** The issue as it is opened. */
export function issue(report, id, pictureUrl) {
  const lines = [scrub(report.body)];
  if (pictureUrl) lines.push(`![The window](${pictureUrl})`);
  lines.push(
    `<sub>${id} · ${report.agent ? 'drafted by an agent, sent by the reporter' : 'from the app'} · ` +
      `GitLance ${report.version || '?'} (${report.commit || '?'}) · installation ${report.install.slice(0, 6)}</sub>`,
  );
  return {
    title: scrub(report.title),
    body: lines.join('\n\n'),
    labels: report.agent ? ['from-app', 'agent'] : ['from-app'],
  };
}

/** The same problem reported again: a comment on the first one. */
export function again(report, id) {
  return `Reported again — ${id}, GitLance ${report.version || '?'} (${report.commit || '?'}), ` +
    `installation ${report.install.slice(0, 6)}:\n\n${scrub(report.body)}`;
}

/** What became of an issue, as the app says it. */
export function state(issue, intake) {
  const repo = (issue.repository_url || '').split('/repos/')[1];
  if (repo && repo.toLowerCase() !== intake.toLowerCase()) return { state: 'moved to the project', url: issue.html_url };
  const labels = (issue.labels || []).map((l) => (typeof l === 'string' ? l : l.name));
  if (labels.includes('duplicate')) return { state: 'duplicate', url: '' };
  if (issue.state === 'closed') return { state: issue.state_reason === 'completed' ? 'fixed' : 'closed', url: '' };
  return { state: 'received', url: '' };
}
