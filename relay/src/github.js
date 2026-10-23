// GitHub, as the GitHub App the relay is: a short-lived token for its installation on the intake
// repository, and the few calls a report needs. The app's key is a Worker secret; nothing of it
// is in GitLance.

const API = 'https://api.github.com';

function b64url(bytes) {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function der(pem) {
  const body = pem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, '');
  return Uint8Array.from(atob(body), (c) => c.charCodeAt(0));
}

/** The app's own token (a JWT, 9 minutes), signed with its key (PKCS#8 PEM). */
export async function appJwt(appId, pem, now = Math.floor(Date.now() / 1000)) {
  if (pem.includes('BEGIN RSA PRIVATE KEY')) {
    throw new Error('The app key is PKCS#1: convert it to PKCS#8 first (relay/README.md)');
  }
  const key = await crypto.subtle.importKey(
    'pkcs8', der(pem), { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, false, ['sign'],
  );
  const part = (o) => b64url(new TextEncoder().encode(JSON.stringify(o)));
  const head = part({ alg: 'RS256', typ: 'JWT' });
  const claims = part({ iat: now - 60, exp: now + 540, iss: String(appId) });
  const sig = await crypto.subtle.sign('RSASSA-PKCS1-v1_5', key, new TextEncoder().encode(`${head}.${claims}`));
  return `${head}.${claims}.${b64url(new Uint8Array(sig))}`;
}

function headers(token) {
  return {
    Authorization: `Bearer ${token}`,
    Accept: 'application/vnd.github+json',
    'User-Agent': 'gitlance-reports-relay',
    'X-GitHub-Api-Version': '2022-11-28',
  };
}

export class GitHub {
  constructor(env, kv) {
    this.env = env;
    this.kv = kv;
    this.repo = env.REPO;
  }

  /** The installation's token, kept until a minute before it ends. */
  async token() {
    const kept = await this.kv.get('gh:token', 'json');
    if (kept && kept.until > Date.now() + 60_000) return kept.token;
    const jwt = await appJwt(this.env.APP_ID, this.env.APP_KEY);
    let installation = this.env.INSTALLATION_ID || (await this.kv.get('gh:installation'));
    if (!installation) {
      const r = await fetch(`${API}/repos/${this.repo}/installation`, { headers: headers(jwt) });
      if (!r.ok) throw new Error(`The app is not installed on ${this.repo}: ${r.status}`);
      installation = String((await r.json()).id);
      await this.kv.put('gh:installation', installation);
    }
    const r = await fetch(`${API}/app/installations/${installation}/access_tokens`, {
      method: 'POST',
      headers: headers(jwt),
    });
    if (!r.ok) throw new Error(`GitHub refused the app: ${r.status}`);
    const t = await r.json();
    await this.kv.put('gh:token', JSON.stringify({ token: t.token, until: Date.parse(t.expires_at) }), {
      expirationTtl: 3000,
    });
    return t.token;
  }

  async call(method, path, body) {
    const token = await this.token();
    const r = await fetch(`${API}${path}`, {
      method,
      headers: body ? { ...headers(token), 'Content-Type': 'application/json' } : headers(token),
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!r.ok) throw new Error(`GitHub: ${method} ${path} → ${r.status}`);
    return r.status === 204 ? null : r.json();
  }

  createIssue(issue) {
    return this.call('POST', `/repos/${this.repo}/issues`, issue);
  }

  comment(number, body) {
    return this.call('POST', `/repos/${this.repo}/issues/${number}/comments`, { body });
  }

  /** An issue by its number in the intake; one moved to the project is followed there. */
  getIssue(number) {
    return this.call('GET', `/repos/${this.repo}/issues/${number}`);
  }

  /** The window's picture, kept in the intake beside its issue. */
  putPicture(id, base64) {
    return this.call('PUT', `/repos/${this.repo}/contents/attachments/${id}.png`, {
      message: `The picture of ${id}`,
      content: base64,
    });
  }
}
