import { test } from 'node:test';
import assert from 'node:assert/strict';
import { check, issue, reportId, scrub, state } from '../src/report.js';

const good = { install: 'abcdef0123456789', version: '0.2.0', commit: '032e411', title: ' Empty  diff ', body: 'It broke.', fingerprint: 'abc123' };

test('a report is checked and trimmed', () => {
  const r = check(good);
  assert.equal(r.ok, true);
  assert.equal(r.report.title, 'Empty diff');
  assert.equal(check({ ...good, title: '' }).ok, false);
  assert.equal(check({ ...good, install: 'x' }).ok, false);
  assert.equal(check({ ...good, picture: 'not a png' }).ok, false);
  assert.equal(check({ ...good, picture: 'iVBORw0KGgo=' }).ok, true);
  assert.equal(check(null).ok, false);
});

test('nothing private goes, and nobody is mentioned', () => {
  assert.equal(
    scrub('at /Users/kestrel/Projects/x, token ghp_abcdefghijklmnopqrstuvwx, ask kestrel@example.com or @juniper'),
    'at ~/Projects/x, token <token>, ask <email> or @​juniper',
  );
  assert.equal(scrub('`@x` stays code'), '`@x` stays code');
});

test('the issue says where it came from', () => {
  const r = check({ ...good, agent: true }).report;
  const made = issue(r, 'R-7F3A2C', 'https://x/p.png');
  assert.equal(made.title, 'Empty diff');
  assert.deepEqual(made.labels, ['from-app', 'agent']);
  assert.match(made.body, /^It broke\.\n\n!\[The window\]\(https:\/\/x\/p\.png\)\n\n<sub>R-7F3A2C · drafted by an agent/);
  assert.match(made.body, /installation abcdef<\/sub>$/);
});

test('ids look alike', () => {
  assert.equal(reportId(new Uint8Array([0x7f, 0x3a, 0x2c])), 'R-7F3A2C');
  assert.match(reportId(), /^R-[0-9A-F]{6}$/);
});

test('what became of an issue', () => {
  const intake = 'Thernal/gitlance-reports';
  const at = (repo) => `https://api.github.com/repos/${repo}`;
  assert.deepEqual(state({ repository_url: at(intake), state: 'open', labels: [] }, intake), { state: 'received', url: '' });
  assert.deepEqual(
    state({ repository_url: at('Thernal/gitlance-desktop'), state: 'open', html_url: 'https://github.com/Thernal/gitlance-desktop/issues/142' }, intake),
    { state: 'moved to the project', url: 'https://github.com/Thernal/gitlance-desktop/issues/142' },
  );
  assert.equal(state({ repository_url: at(intake), state: 'open', labels: [{ name: 'duplicate' }] }, intake).state, 'duplicate');
  assert.equal(state({ repository_url: at(intake), state: 'closed', state_reason: 'completed' }, intake).state, 'fixed');
});
