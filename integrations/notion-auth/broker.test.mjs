import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createNotionBroker } from './broker.mjs';

const origin = 'https://auth.example.com';
const verifier = 'v'.repeat(43);
const challenge = createHash('sha256').update(verifier).digest('base64url');
const tokenReply = { access_token: 'ntn_access', refresh_token: 'nrt_refresh', token_type: 'bearer',
  workspace_id: '00000000-0000-4000-8000-000000000001', workspace_name: 'Workspace', owner: { person: { email: 'private@example.com' } } };
function setup(extra = {}) {
  const requests = [];
  const handle = createNotionBroker({ publicOrigin: origin, clientId: 'client-id', clientSecret: 'server-secret',
    fetch: async (url, options) => { requests.push({ url, ...options, payload: JSON.parse(options.body) }); return Response.json(tokenReply); }, ...extra });
  const post = (path, body, headers = {}) => handle(new Request(`${origin}/v1/notion/${path}`, {
    method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body: JSON.stringify(body),
  }));
  const start = async () => {
    const response = await post('start', { challenge }); assert.equal(response.status, 200);
    return response.json();
  };
  const callback = (flow, params = { code: 'temporary-code' }) => {
    const query = new URLSearchParams({ state: new URL(flow.authorizationUrl).searchParams.get('state'), ...params });
    return handle(new Request(`${origin}/v1/notion/callback?${query}`));
  };
  return { handle, post, start, callback, requests };
}
test('full public OAuth flow binds code to native proof and exposes no tokens in URLs', async () => {
  const { start, post, callback, requests } = setup();
  const flow = await start();
  assert.equal(flow.transactionId.length, 43);
  const url = new URL(flow.authorizationUrl);
  assert.equal(url.origin + url.pathname, 'https://api.notion.com/v1/oauth/authorize');
  assert.equal(url.searchParams.get('redirect_uri'), `${origin}/v1/notion/callback`);
  assert.equal(url.searchParams.get('owner'), 'user'); assert.equal(url.searchParams.get('response_type'), 'code');
  assert.equal(url.searchParams.has('code_challenge'), false); // S256 belongs to broker handoff, not Notion.
  assert.equal(url.href.includes(verifier), false); assert.equal(url.href.includes('server-secret'), false);
  const proof = { transactionId: flow.transactionId, verifier };
  assert.equal((await post('complete', proof)).status, 202);
  const page = await callback(flow);
  assert.equal(page.headers.get('Referrer-Policy'), 'no-referrer'); assert.equal(page.headers.get('Cache-Control'), 'no-store');
  assert.equal((await page.text()).includes('temporary-code'), false);
  const response = await post('complete', proof); const saved = await response.json();
  assert.equal(saved.access_token, tokenReply.access_token); assert.equal(saved.refresh_token, tokenReply.refresh_token);
  assert.equal(saved.owner, undefined); assert.equal(JSON.stringify(saved).includes('private@example.com'), false);
  assert.equal(requests.length, 1); assert.equal(requests[0].url, 'https://api.notion.com/v1/oauth/token');
  assert.equal(requests[0].redirect, 'manual');
  assert.deepEqual(requests[0].payload, { grant_type: 'authorization_code', code: 'temporary-code', redirect_uri: `${origin}/v1/notion/callback` });
  assert.equal(requests[0].headers.Authorization, `Basic ${Buffer.from('client-id:server-secret').toString('base64')}`);
  assert.equal((await post('complete', proof)).status, 410);
});
test('intercepted transaction id cannot retrieve or cancel another native flow', async () => {
  const { start, post, callback, requests } = setup(); const flow = await start(); await callback(flow);
  for (const path of ['complete', 'cancel']) {
    assert.equal((await post(path, { transactionId: flow.transactionId, verifier: 'wrong'.repeat(11) })).status, 410);
  }
  assert.equal(requests.length, 0);
  assert.equal((await post('complete', { transactionId: flow.transactionId, verifier })).status, 200);
});
test('unrelated or duplicate callback state cannot replace the authorized code', async () => {
  const { handle, start, callback, post, requests } = setup(); const flow = await start();
  assert.equal((await callback(flow, { state: 'unknown', code: 'bad' })).status, 400);
  assert.equal((await callback(flow, { state: 'é'.repeat(43), code: 'bad' })).status, 400);
  const state = new URL(flow.authorizationUrl).searchParams.get('state');
  assert.equal((await handle(new Request(`${origin}/v1/notion/callback?state=${state}&state=${state}&code=bad`))).status, 400);
  assert.equal((await callback(flow, { code: 'right' })).status, 200);
  assert.equal((await callback(flow, { code: 'replacement' })).status, 400);
  await post('complete', { transactionId: flow.transactionId, verifier });
  assert.equal(requests[0].payload.code, 'right');
});
test('denial and cancellation preserve one-time semantics without exchanging codes', async () => {
  const { start, callback, post, requests } = setup();
  const denied = await start(); await callback(denied, { error: 'access_denied' });
  assert.deepEqual(await (await post('complete', { transactionId: denied.transactionId, verifier })).json(), { denied: true });
  assert.equal((await post('complete', { transactionId: denied.transactionId, verifier })).status, 410);
  const canceled = await start();
  assert.equal((await post('cancel', { transactionId: canceled.transactionId, verifier })).status, 200);
  assert.equal((await callback(canceled)).status, 400); assert.equal(requests.length, 0);
});
test('expired transactions and capacity are bounded and capacity is reclaimed', async () => {
  let now = 0;
  const { start, post, callback, requests } = setup({ now: () => now, ttlMs: 1_000, maxFlows: 1 });
  const expired = await start();
  assert.equal((await post('start', { challenge })).status, 429);
  now = 1_001;
  assert.equal((await callback(expired)).status, 400);
  assert.equal((await post('complete', { transactionId: expired.transactionId, verifier })).status, 410);
  await start(); assert.equal(requests.length, 0);
});
test('concurrent completion exchanges a code only once', async () => {
  let release;
  let calls = 0;
  const { start, post, callback } = setup({ fetch: async () => {
    calls++; await new Promise(resolve => { release = resolve; }); return Response.json(tokenReply);
  } });
  const flow = await start(); await callback(flow);
  const proof = { transactionId: flow.transactionId, verifier };
  const first = post('complete', proof);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal((await post('complete', proof)).status, 410);
  release(); assert.equal((await first).status, 200); assert.equal(calls, 1);
});
test('refresh rotates both credentials through the documented fixed endpoint', async () => {
  const { post, requests } = setup();
  const result = await (await post('refresh', { refreshToken: 'nrt_old', endpoint: 'https://evil.example', clientSecret: 'injected' })).json();
  assert.equal(result.refresh_token, 'nrt_refresh');
  assert.equal(requests[0].url, 'https://api.notion.com/v1/oauth/token');
  assert.deepEqual(requests[0].payload, { grant_type: 'refresh_token', refresh_token: 'nrt_old' });
});
test('revocation requires existing token possession and uses the documented endpoint', async () => {
  const { post, requests } = setup();
  assert.equal((await post('revoke', {})).status, 400);
  assert.deepEqual(await (await post('revoke', { token: 'ntn_existing' })).json(), { revoked: true });
  assert.equal(requests[0].url, 'https://api.notion.com/v1/oauth/revoke'); assert.deepEqual(requests[0].payload, { token: 'ntn_existing' });
});
test('native API rejects browser origins, unsafe bodies and caller-selected redirects', async () => {
  const { start, post, handle } = setup();
  assert.equal((await post('start', { challenge }, { Origin: 'https://evil.example' })).status, 400);
  assert.equal((await handle(new Request(`${origin}/v1/notion/start`, { method: 'POST', body: 'challenge=x' }))).status, 400);
  assert.equal((await post('start', { challenge: 'bad' })).status, 400);
  assert.equal((await post('start', { challenge, padding: 'x'.repeat(16_384) })).status, 502);
  const flow = await start();
  const started = await (await post('start', { challenge, redirect_uri: 'https://evil.example', returnTo: 'http://127.0.0.1:8000' })).json();
  assert.equal(new URL(started.authorizationUrl).searchParams.get('redirect_uri'), `${origin}/v1/notion/callback`);
  assert.equal(new URL(flow.authorizationUrl).searchParams.has('access_token'), false);
  assert.equal((await handle(new Request('https://evil.example/v1/notion/start', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ challenge }) }))).status, 400);
});
test('provider failures are redacted and consumed transactions are never replayed', async () => {
  const { start, callback, post } = setup({ fetch: async () => new Response('private provider secret detail', { status: 401 }) });
  const flow = await start(); await callback(flow); const proof = { transactionId: flow.transactionId, verifier };
  const response = await post('complete', proof);
  assert.equal(response.status, 502); assert.deepEqual(await response.json(), { error: 'connection_failed' });
  assert.equal((await post('complete', proof)).status, 410);
});
test('oversized and malformed provider token responses never reach the desktop', async () => {
  for (const value of [{ ...tokenReply, refresh_token: 5 }, { ...tokenReply, workspace_id: 'invalid' }, { ...tokenReply, access_token: 'token\nheader' }, { ...tokenReply, huge: 'x'.repeat(65_536) }]) {
    const { post } = setup({ fetch: async () => Response.json(value) });
    assert.equal((await post('refresh', { refreshToken: 'nrt_old' })).status, 502);
  }
});
test('nullable refresh credential remains a valid connected access token', async () => {
  const { start, callback, post } = setup({ fetch: async () => Response.json({ ...tokenReply, refresh_token: null }) });
  const flow = await start(); await callback(flow);
  const response = await post('complete', { transactionId: flow.transactionId, verifier });
  assert.equal(response.status, 200);
  assert.equal((await response.json()).refresh_token, null);
});
test('configuration cannot expand TTL, state capacity, provider endpoint or redirect origin', () => {
  for (const extra of [{ publicOrigin: 'http://auth.example.com' }, { publicOrigin: `${origin}/path` }, { publicOrigin: 'https://user@auth.example.com' }, { maxFlows: 1_001 }, { ttlMs: 300_001 }, { clientSecret: '' }]) {
    assert.throws(() => setup(extra), /configuration/);
  }
});
