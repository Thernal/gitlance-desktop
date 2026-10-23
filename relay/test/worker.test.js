import { test } from 'node:test';
import assert from 'node:assert/strict';
import worker from '../src/worker.js';
import { appJwt } from '../src/github.js';

/** A KV in memory. */
function kv() {
  const map = new Map();
  return {
    map,
    async get(key, type) {
      const v = map.get(key);
      return v === undefined ? null : type === 'json' ? JSON.parse(v) : v;
    },
    async put(key, value) {
      map.set(key, value);
    },
  };
}

/** A key like a GitHub App's, as PKCS#8 PEM. */
async function pem() {
  const pair = await crypto.subtle.generateKey(
    { name: 'RSASSA-PKCS1-v1_5', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: 'SHA-256' },
    true,
    ['sign', 'verify'],
  );
  const der = new Uint8Array(await crypto.subtle.exportKey('pkcs8', pair.privateKey));
  const b64 = btoa(String.fromCharCode(...der));
  return { text: `-----BEGIN PRIVATE KEY-----\n${b64}\n-----END PRIVATE KEY-----\n`, pair };
}

/** GitHub as far as the relay sees it: what it was asked, and issues it keeps. */
function github() {
  const asked = [];
  const issues = new Map();
  globalThis.fetch = async (url, init = {}) => {
    const method = init.method || 'GET';
    const path = new URL(url).pathname;
    asked.push(`${method} ${path}`);
    const body = init.body ? JSON.parse(init.body) : null;
    const reply = (v, status = 200) => new Response(JSON.stringify(v), { status });
    if (path === '/repos/Thernal/gitlance-reports/installation') return reply({ id: 77 });
    if (path === '/app/installations/77/access_tokens') {
      return reply({ token: 'ghs_x', expires_at: new Date(Date.now() + 3_600_000).toISOString() }, 201);
    }
    if (method === 'POST' && path === '/repos/Thernal/gitlance-reports/issues') {
      const number = issues.size + 1;
      issues.set(number, { ...body, number, state: 'open', repository_url: 'https://api.github.com/repos/Thernal/gitlance-reports', comments: [] });
      return reply({ number }, 201);
    }
    let m = path.match(/^\/repos\/Thernal\/gitlance-reports\/issues\/(\d+)$/);
    if (m) return issues.has(+m[1]) ? reply(issues.get(+m[1])) : reply({}, 404);
    m = path.match(/^\/repos\/Thernal\/gitlance-reports\/issues\/(\d+)\/comments$/);
    if (m && method === 'POST') {
      issues.get(+m[1]).comments.push(body.body);
      return reply({}, 201);
    }
    if (method === 'PUT' && path.startsWith('/repos/Thernal/gitlance-reports/contents/attachments/')) return reply({}, 201);
    return reply({ message: 'not stubbed' }, 500);
  };
  return { asked, issues };
}

async function setup(extra = {}) {
  const key = await pem();
  return { env: { REPO: 'Thernal/gitlance-reports', OPEN: '1', DAILY_CAP: '100', APP_ID: '123', APP_KEY: key.text, REPORTS: kv(), ...extra }, key };
}

const send = (env, body, ip = '1.2.3.4') =>
  worker.fetch(new Request('https://relay.test/v1/report', { method: 'POST', body: JSON.stringify(body), headers: { 'CF-Connecting-IP': ip } }), env);
const report = { install: 'abcdef0123456789', version: '0.2.0', commit: '032e411', title: 'Empty diff', body: 'It broke at /Users/kestrel/x.', fingerprint: 'f00d' };

test('the app key signs a JWT GitHub can check', async () => {
  const { text, pair } = await pem();
  const jwt = await appJwt('123', text, 1_000_000);
  const [head, claims, sig] = jwt.split('.');
  const fromB64url = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));
  assert.deepEqual(JSON.parse(new TextDecoder().decode(fromB64url(claims))), { iat: 999_940, exp: 1_000_540, iss: '123' });
  const ok = await crypto.subtle.verify('RSASSA-PKCS1-v1_5', pair.publicKey, fromB64url(sig), new TextEncoder().encode(`${head}.${claims}`));
  assert.equal(ok, true);
  await assert.rejects(appJwt('1', '-----BEGIN RSA PRIVATE KEY-----\nx\n-----END RSA PRIVATE KEY-----'), /PKCS#8/);
});

test('a report becomes an issue, scrubbed; the same again a comment on it', async () => {
  const { env } = await setup();
  const gh = github();
  const first = await send(env, report);
  assert.equal(first.status, 200);
  const { id } = await first.json();
  assert.match(id, /^R-[0-9A-F]{6}$/);
  const made = gh.issues.get(1);
  assert.equal(made.title, 'Empty diff');
  assert.match(made.body, /It broke at ~\/x\./);
  const again = await send(env, { ...report, body: 'Again.' });
  assert.equal(again.status, 200);
  assert.equal(gh.issues.size, 1, 'no second issue');
  assert.match(gh.issues.get(1).comments[0], /Reported again — R-/);
  // Its state, for the installation that sent it only.
  const mine = await worker.fetch(new Request(`https://relay.test/v1/report/${id}?install=${report.install}`), env);
  assert.deepEqual(await mine.json(), { state: 'received', url: '' });
  const theirs = await worker.fetch(new Request(`https://relay.test/v1/report/${id}?install=someoneelse12`), env);
  assert.equal(theirs.status, 404);
});

test('a picture is kept beside its issue', async () => {
  const { env } = await setup();
  const gh = github();
  const r = await send(env, { ...report, fingerprint: '', picture: 'iVBORw0KGgo=' });
  const { id } = await r.json();
  assert.ok(gh.asked.includes(`PUT /repos/Thernal/gitlance-reports/contents/attachments/${id}.png`));
  assert.match(gh.issues.get(1).body, new RegExp(`attachments/${id}\\.png\\?raw=true`));
});

test('limits: too many from one installation, closed, not a report', async () => {
  const { env } = await setup();
  github();
  for (let n = 0; n < 10; n++) assert.equal((await send(env, { ...report, fingerprint: '' })).status, 200);
  assert.equal((await send(env, { ...report, fingerprint: '' })).status, 429);
  const closed = await setup({ OPEN: '0' });
  assert.equal((await send(closed.env, report)).status, 503);
  assert.equal((await send(env, { title: 'x' })).status, 400);
});
