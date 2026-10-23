import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { Miniflare, convertV4MiniflareOptions } from 'miniflare';

const origin = 'https://auth.example.com';
const verifier = 'v'.repeat(43);
const challenge = createHash('sha256').update(verifier).digest('base64url');
const tokenReply = { access_token: 'ntn_worker_access', refresh_token: 'nrt_worker_refresh', token_type: 'bearer',
  workspace_id: '00000000-0000-4000-8000-000000000001', workspace_name: 'Worker test' };
function setup(extra = {}) {
  const requests = [];
  const mf = new Miniflare(convertV4MiniflareOptions({ name: 'tuck-notion-auth-test',
    modules: ['worker.mjs', 'broker.mjs'].map(name => ({ type: 'ESModule', path: fileURLToPath(new URL(name, import.meta.url)) })),
    modulesRoot: fileURLToPath(new URL('.', import.meta.url)),
    compatibilityDate: '2026-10-23',
    durableObjects: { NOTION_FLOWS: { className: 'NotionFlow', useSQLite: true } },
    bindings: { TUCK_NOTION_AUTH_PUBLIC_ORIGIN: origin, NOTION_OAUTH_CLIENT_ID: 'worker-client', NOTION_OAUTH_CLIENT_SECRET: 'fake-worker-secret' },
    ratelimits: {
      NOTION_AUTH_RATE_LIMIT: { namespace_id: '1001', simple: { limit: 20, period: 60 } },
      NOTION_POLL_RATE_LIMIT: { namespace_id: '1002', simple: { limit: 240, period: 60 } },
      NOTION_GLOBAL_RATE_LIMIT: { namespace_id: '1003', simple: { limit: 600, period: 60 } },
    },
    outboundService: async request => {
      requests.push({ url: request.url, body: await request.json() });
      return Response.json(tokenReply);
    }, ...extra,
  }));
  const post = (path, body) => mf.dispatchFetch(`${origin}/v1/notion/${path}`, {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': '192.0.2.1' }, body: JSON.stringify(body),
  });
  return { mf, requests, post };
}
test('real Worker and SQLite Durable Object complete one protected authorization', async () => {
  const { mf, requests, post } = setup();
  try {
    const response = await post('start', { challenge }); assert.equal(response.status, 200);
    const flow = await response.json();
    const state = new URL(flow.authorizationUrl).searchParams.get('state');
    assert.equal(state, flow.transactionId);
    const proof = { transactionId: flow.transactionId, verifier };
    assert.equal((await post('complete', proof)).status, 202);
    assert.equal((await mf.dispatchFetch(`${origin}/v1/notion/callback?state=${state}&code=worker-code`)).status, 200);
    await mf.unsafeEvictDurableObject('tuck-notion-auth-test', 'NotionFlow', { name: state });
    const completions = await Promise.all([post('complete', proof), post('complete', proof)]);
    assert.deepEqual(completions.map(response => response.status).sort(), [200, 410]);
    const success = completions.find(response => response.status === 200);
    assert.deepEqual(await success.json(), tokenReply);
    assert.equal(requests.length, 1); assert.equal(requests[0].body.code, 'worker-code');
    assert.equal(requests[0].url, 'https://api.notion.com/v1/oauth/token');
    assert.equal((await mf.dispatchFetch(`${origin}/v1/notion/callback?state=${state}&code=replay`)).status, 400);
  } finally { await mf.dispose(); }
});
test('Durable Object enforces expiry and verifier without provider calls', async () => {
  const { mf, requests, post } = setup();
  try {
    const flow = await (await post('start', { challenge })).json();
    assert.equal((await post('complete', { transactionId: flow.transactionId, verifier: 'x'.repeat(43) })).status, 410);
    assert.equal((await post('cancel', { transactionId: flow.transactionId, verifier })).status, 200);
    assert.equal((await post('complete', { transactionId: flow.transactionId, verifier })).status, 410);
    const namespace = await mf.getDurableObjectNamespace('NOTION_FLOWS');
    const id = 'e'.repeat(43);
    const stub = namespace.getByName(id);
    assert.equal(await stub.create({ challenge, expires: Date.now() + 40 }), true);
    await new Promise(resolve => setTimeout(resolve, 60));
    assert.equal(await stub.accept({ code: 'expired-code' }), false);
    assert.equal((await stub.consume(challenge, false)).gone, true);
    assert.equal(requests.length, 0);
  } finally { await mf.dispose(); }
});
test('Worker rate limit binding rejects excessive authorization requests', async () => {
  const { mf, post } = setup();
  try {
    const statuses = [];
    for (let index = 0; index < 25; index++) statuses.push((await post('start', { challenge })).status);
    assert.ok(statuses.includes(429));
  } finally { await mf.dispose(); }
});
test('health reveals only readiness and missing registration fails closed', async () => {
  const { mf, post, requests } = setup({ bindings: {} });
  try {
    const health = await mf.dispatchFetch(`${origin}/health`);
    assert.equal(health.status, 200); assert.deepEqual(await health.json(), { configured: false });
    assert.equal((await post('start', { challenge })).status, 503);
    assert.equal(requests.length, 0);
  } finally { await mf.dispose(); }
});
