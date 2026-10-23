// The relay between GitLance and the project's private reports (Desktop #118): it takes a report
// from the app, checks it, counts it against the day's limits, and files it as an issue in the
// intake repository as the GitHub App — or, when the same problem is already open there, as a
// comment on it. GitLance holds no key; this is the only place one is.
//
// POST /v1/report              → { id }           (400 not a report, 413 too large, 429 too many, 503 closed)
// GET  /v1/report/<id>?install → { state, url }   (only for the installation that sent it)

import { LIMITS, again, check, issue, reportId, state } from './report.js';
import { GitHub } from './github.js';

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    try {
      if (request.method === 'POST' && url.pathname === '/v1/report') return await receive(request, env);
      const found = url.pathname.match(/^\/v1\/report\/(R-[0-9A-F]{6})$/);
      if (request.method === 'GET' && found) {
        return await status(found[1], url.searchParams.get('install') || '', env);
      }
      if (request.method === 'GET' && url.pathname === '/') return json({ open: env.OPEN !== '0' });
      return json({ error: 'Nothing here' }, 404);
    } catch (e) {
      console.error(e);
      return json({ error: 'The reports could not take it now' }, 502);
    }
  },
};

function json(value, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

async function receive(request, env) {
  // The kill switch: OPEN = "0" in wrangler.toml, or as a secret.
  if (env.OPEN === '0') return json({ error: 'The reports are closed for now' }, 503);
  if (Number(request.headers.get('Content-Length') || 0) > LIMITS.picture + LIMITS.body + 20_000) {
    return json({ error: 'The report is too large' }, 413);
  }
  let payload;
  try {
    payload = await request.json();
  } catch {
    return json({ error: 'Not a report' }, 400);
  }
  const checked = check(payload);
  if (!checked.ok) return json({ error: checked.error }, 400);
  const report = checked.report;

  const kv = env.REPORTS;
  const day = new Date().toISOString().slice(0, 10);
  const address = request.headers.get('CF-Connecting-IP') || 'unknown';
  const counts = [
    [`rate:i:${report.install}:${day}`, LIMITS.perInstall],
    [`rate:a:${address}:${day}`, LIMITS.perAddress],
    [`rate:all:${day}`, Number(env.DAILY_CAP || 100)],
  ];
  const now = [];
  for (const [key, most] of counts) {
    const n = Number((await kv.get(key)) || 0);
    if (n >= most) return json({ error: 'Too many reports today' }, 429);
    now.push(n);
  }
  for (const [i, [key]] of counts.entries()) {
    await kv.put(key, String(now[i] + 1), { expirationTtl: 2 * 86_400 });
  }

  const gh = new GitHub(env, kv);
  const id = reportId();
  // The same problem again, while the first is open in the intake: a comment on it.
  if (report.fingerprint) {
    const first = await kv.get(`fp:${report.fingerprint}`);
    const open = first && (await gh.getIssue(first).catch(() => null));
    if (open && state(open, env.REPO).state === 'received') {
      await gh.comment(first, again(report, id));
      await kv.put(`id:${id}`, JSON.stringify({ issue: Number(first), install: report.install }));
      return json({ id });
    }
  }
  let picture = null;
  if (report.picture) {
    await gh.putPicture(id, report.picture);
    picture = `https://github.com/${env.REPO}/blob/main/attachments/${id}.png?raw=true`;
  }
  const made = await gh.createIssue(issue(report, id, picture));
  await kv.put(`id:${id}`, JSON.stringify({ issue: made.number, install: report.install }));
  if (report.fingerprint) {
    await kv.put(`fp:${report.fingerprint}`, String(made.number), { expirationTtl: 30 * 86_400 });
  }
  return json({ id });
}

async function status(id, install, env) {
  const kept = await env.REPORTS.get(`id:${id}`, 'json');
  if (!kept || kept.install !== install) return json({ error: 'No such report' }, 404);
  const found = await new GitHub(env, env.REPORTS).getIssue(kept.issue);
  return json(state(found, env.REPO));
}
