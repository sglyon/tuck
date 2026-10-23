// Thin confidential-client edge. The desktop owns saved credentials and all meeting data.
// Only short-lived OAuth codes and S256 handoff challenges cross the flow store seam.

const TOKEN_URL = 'https://api.notion.com/v1/oauth/token';
const REVOKE_URL = 'https://api.notion.com/v1/oauth/revoke';
const AUTHORIZE_URL = 'https://api.notion.com/v1/oauth/authorize';
const TTL = 300_000;
const MAX_BODY = 16_384;
const MAX_RESPONSE = 65_536;
const headers = {
  'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer',
  'X-Content-Type-Options': 'nosniff',
  'Content-Security-Policy': "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
};
const json = (value, status = 200) => new Response(JSON.stringify(value), {
  status, headers: { ...headers, 'Content-Type': 'application/json' },
});
const fail = (status = 400) => json({ error: 'connection_failed' }, status);
const base64 = bytes => btoa(String.fromCharCode(...bytes));
const base64url = bytes => base64(bytes).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
const random = () => base64url(crypto.getRandomValues(new Uint8Array(32)));
const digest = async value => base64url(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(value))));
const validOpaque = value => typeof value === 'string' && /^[A-Za-z0-9_-]{43,128}$/.test(value);
const validToken = value => typeof value === 'string' && value.length > 0 && value.length <= 8_192 && !/[\s\x00-\x1f\x7f]/.test(value);

export async function boundedText(stream, maximum) {
  if (!stream) return '';
  const reader = stream.getReader();
  let bytes = 0;
  const chunks = [];
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > maximum) throw new Error('response_too_large');
      chunks.push(value);
    }
    const joined = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
    return new TextDecoder('utf-8', { fatal: true }).decode(joined);
  } finally { await reader.cancel().catch(() => {}); }
}

// Fetch is injected for offline contract tests; provider endpoints are never configurable.
export function createNotionBroker({ publicOrigin, clientId, clientSecret, fetch: send = fetch,
  now = Date.now, maxFlows = 1_000, ttlMs = TTL, flows = memoryFlowStore({ now, maxFlows }) }) {
  const origin = new URL(publicOrigin);
  if (origin.protocol !== 'https:' || origin.origin !== publicOrigin || origin.username || origin.password
    || !clientId || !clientSecret || /[\r\n:]/.test(clientId)
    || !Number.isInteger(maxFlows) || maxFlows < 1 || maxFlows > 1_000
    || !Number.isInteger(ttlMs) || ttlMs < 1 || ttlMs > TTL) throw new Error('Invalid broker configuration');
  const callback = `${origin.origin}/v1/notion/callback`;
  const provider = async (endpoint, payload) => {
    const response = await send(endpoint, {
      // Workers supports manual/follow only. A 3xx is rejected below, never followed.
      method: 'POST', redirect: 'manual', signal: AbortSignal.timeout(15_000),
      headers: { Authorization: `Basic ${base64(new TextEncoder().encode(`${clientId}:${clientSecret}`))}`,
        'Content-Type': 'application/json', 'Notion-Version': '2026-03-11' },
      body: JSON.stringify(payload),
    });
    const raw = await boundedText(response.body, MAX_RESPONSE);
    if (!response.ok) throw new Error('provider_failed');
    return JSON.parse(raw);
  };
  const tokens = value => {
    if (!validToken(value.access_token) || (value.refresh_token !== null && value.refresh_token !== undefined && !validToken(value.refresh_token))
      || value.token_type !== 'bearer' || typeof value.workspace_id !== 'string'
      || !/^[0-9a-f-]{36}$/i.test(value.workspace_id)) throw new Error('invalid_tokens');
    const result = { access_token: value.access_token, refresh_token: value.refresh_token ?? null,
      token_type: 'bearer', workspace_id: value.workspace_id,
      workspace_name: typeof value.workspace_name === 'string' ? value.workspace_name.slice(0, 256) : null };
    // Notion currently does not document an expiry. Accept one if the provider supplies it.
    if (Number.isSafeInteger(value.expires_in) && value.expires_in > 0 && value.expires_in <= 31_536_000)
      result.expires_in = value.expires_in;
    return result;
  };
  return async function handle(request) {
    const url = new URL(request.url);
    if (url.origin !== origin.origin || url.href.length > 16_384) return fail();
    if (request.method === 'GET' && url.pathname === '/v1/notion/callback') {
      const state = url.searchParams.get('state');
      const code = url.searchParams.get('code');
      const denied = url.searchParams.get('error');
      if (!validOpaque(state)
        || url.searchParams.getAll('state').length !== 1
        || url.searchParams.getAll('code').length > 1 || url.searchParams.getAll('error').length > 1
        || (!!code === !!denied) || (code && (!validToken(code) || code.length > 4_096))) return fail();
      if (!await flows.accept(state, denied ? { denied: true } : { code })) return fail();
      return new Response('<!doctype html><meta charset="utf-8"><title>Tuck connection</title><h1>Return to Tuck</h1><p>You can close this tab and return to the app.</p>', {
        headers: { ...headers, 'Content-Type': 'text/html; charset=utf-8' },
      });
    }
    // Native-only JSON API: no CORS, cookies, browser-origin requests or caller-selected redirects.
    if (request.method !== 'POST' || request.headers.has('Origin') || url.search
      || !request.headers.get('Content-Type')?.toLowerCase().startsWith('application/json')) return fail();
    try {
      const body = JSON.parse(await boundedText(request.body, MAX_BODY));
      if (!body || typeof body !== 'object' || Array.isArray(body)) return fail();
      if (url.pathname === '/v1/notion/start') {
        if (!validOpaque(body.challenge) || body.challenge.length !== 43) return fail();
        const id = random(), state = id; // random state routes directly to its one transaction DO
        if (!await flows.create(id, { challenge: body.challenge, expires: now() + ttlMs })) return fail(429);
        const authorization = new URL(AUTHORIZE_URL);
        authorization.search = new URLSearchParams({ owner: 'user', client_id: clientId,
          redirect_uri: callback, response_type: 'code', state }).toString();
        return json({ transactionId: id, authorizationUrl: authorization.href, expiresIn: Math.floor(ttlMs / 1_000) });
      }
      if (['/v1/notion/complete', '/v1/notion/cancel'].includes(url.pathname)) {
        if (!validOpaque(body.transactionId) || !validOpaque(body.verifier)) return fail();
        const result = await flows.consume(body.transactionId, await digest(body.verifier), url.pathname.endsWith('/cancel'));
        if (result.gone) return fail(410);
        if (result.canceled) return json({ canceled: true });
        if (result.pending) return json({ pending: true }, 202);
        if (result.denied) return json({ denied: true });
        return json(tokens(await provider(TOKEN_URL, { grant_type: 'authorization_code',
          code: result.code, redirect_uri: callback })));
      }
      if (url.pathname === '/v1/notion/refresh') {
        if (!validToken(body.refreshToken)) return fail();
        return json(tokens(await provider(TOKEN_URL, { grant_type: 'refresh_token', refresh_token: body.refreshToken })));
      }
      if (url.pathname === '/v1/notion/revoke') {
        if (!validToken(body.token)) return fail();
        await provider(REVOKE_URL, { token: body.token });
        return json({ revoked: true });
      }
      return fail(404);
    } catch { return fail(502); }
  };
}

// Offline protocol tests use the same atomic store contract as the per-flow Durable Object.
// Production always supplies DurableFlowStore; this bounded map is never Worker state.
export function memoryFlowStore({ now = Date.now, maxFlows = 1_000 } = {}) {
  const records = new Map();
  const prune = () => { for (const [id, flow] of records) if (flow.expires <= now()) records.delete(id); };
  return {
    create(id, value) { prune(); if (records.size >= maxFlows || records.has(id)) return false; records.set(id, value); return true; },
    accept(id, result) { prune(); const flow = records.get(id); if (!flow || flow.result) return false; flow.result = result; return true; },
    consume(id, challenge, cancel) {
      prune(); const flow = records.get(id);
      if (!flow || !equalChallenge(challenge, flow.challenge)) return { gone: true };
      if (cancel) { records.delete(id); return { canceled: true }; }
      if (!flow.result) return { pending: true };
      records.delete(id); return flow.result;
    },
  };
}
export function equalChallenge(left, right) {
  if (typeof left !== 'string' || typeof right !== 'string' || left.length !== 43 || right.length !== 43) return false;
  const a = new TextEncoder().encode(left), b = new TextEncoder().encode(right);
  if (typeof crypto.subtle.timingSafeEqual === 'function') return crypto.subtle.timingSafeEqual(a, b);
  // Node's WebCrypto lacks the Workers extension; fixed-length fallback is for offline tests.
  let difference = 0;
  for (let index = 0; index < 43; index++) difference |= a[index] ^ b[index];
  return difference === 0;
}
