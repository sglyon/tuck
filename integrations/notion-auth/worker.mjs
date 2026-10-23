import { DurableObject } from 'cloudflare:workers';
import { createNotionBroker, equalChallenge } from './broker.mjs';

// One transaction is the coordination atom. A single SQLite-backed KV record survives
// isolate eviction; consumed authorization codes are deleted before token exchange.
export class NotionFlow extends DurableObject {
  async create(value) {
    if (this.ctx.storage.kv.get('flow') || value.expires <= Date.now() || value.expires > Date.now() + 300_000) return false;
    this.ctx.storage.kv.put('flow', value);
    await this.ctx.storage.setAlarm(value.expires);
    return true;
  }
  current() {
    const value = this.ctx.storage.kv.get('flow');
    if (value && value.expires <= Date.now()) { this.ctx.storage.kv.delete('flow'); return null; }
    return value ?? null;
  }
  accept(result) {
    const value = this.current();
    if (!value || value.result) return false;
    this.ctx.storage.kv.put('flow', { ...value, result });
    return true;
  }
  consume(challenge, cancel) {
    const value = this.current();
    if (!value || !equalChallenge(challenge, value.challenge)) return { gone: true };
    if (!cancel && !value.result) return { pending: true };
    this.ctx.storage.kv.delete('flow'); // synchronous atomic consume before any external fetch
    return cancel ? { canceled: true } : value.result;
  }
  async alarm() { await this.ctx.storage.deleteAll(); }
}

export default {
  async fetch(request, env) {
    try {
      const incoming = new URL(request.url);
      const path = incoming.pathname;
      if (request.method === 'GET' && path === '/health') {
        return Response.json({ configured: Boolean(env.TUCK_NOTION_AUTH_PUBLIC_ORIGIN
          && env.NOTION_OAUTH_CLIENT_ID && env.NOTION_OAUTH_CLIENT_SECRET) }, {
          headers: { 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer' },
        });
      }
      const peer = request.headers.get('CF-Connecting-IP') ?? 'unavailable';
      const expensive = ['/v1/notion/start', '/v1/notion/refresh', '/v1/notion/revoke'].includes(path);
      const global = await env.NOTION_GLOBAL_RATE_LIMIT.limit({ key: expensive ? 'authorization' : 'poll' });
      if (!global.success) return new Response(null, { status: 429, headers: { 'Cache-Control': 'no-store' } });
      const limiter = expensive ? env.NOTION_AUTH_RATE_LIMIT : env.NOTION_POLL_RATE_LIMIT;
      const limited = await limiter.limit({ key: peer });
      if (!limited.success) return new Response(null, { status: 429, headers: { 'Cache-Control': 'no-store' } });
      const stub = id => env.NOTION_FLOWS.getByName(id);
      const flows = {
        create: (id, value) => stub(id).create(value),
        accept: (id, value) => stub(id).accept(value),
        consume: (id, challenge, cancel) => stub(id).consume(challenge, cancel),
      };
      const handle = createNotionBroker({ publicOrigin: env.TUCK_NOTION_AUTH_PUBLIC_ORIGIN,
        clientId: env.NOTION_OAUTH_CLIENT_ID, clientSecret: env.NOTION_OAUTH_CLIENT_SECRET, flows });
      return await handle(request);
    } catch {
      return Response.json({ error: 'connection_unavailable' }, { status: 503,
        headers: { 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer' } });
    }
  },
};
