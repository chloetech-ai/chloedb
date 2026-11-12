// The `mcp` surface: the agent uses ChloeDB the way a developer's coding
// agent does, through the server's Model Context Protocol tools (POST
// /v1/mcp), with the sandbox key the session was given. Every tool the
// server lists becomes a tool of the agent, with one more required field,
// `thought`: what the person is trying and why, which is what makes a
// failure readable afterwards.

const MAX_RESULT = 6000;

export class McpSurface {
  constructor({ url, key, headers = {} }) {
    this.url = url.replace(/\/$/, '');
    this.key = key;
    this.headers = headers;
    this.id = 0;
    this.tools = [];
  }

  async rpc(method, params) {
    const res = await fetch(`${this.url}/v1/mcp`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', authorization: `Bearer ${this.key}`, ...this.headers },
      body: JSON.stringify({ jsonrpc: '2.0', id: ++this.id, method, params }),
      signal: AbortSignal.timeout(120_000),
    });
    const text = await res.text();
    let j;
    try { j = JSON.parse(text); } catch { throw Object.assign(new Error(`MCP ${method}: HTTP ${res.status}: ${text.slice(0, 300)}`), { http: res.status }); }
    if (j.error) throw Object.assign(new Error(`MCP ${method}: ${j.error.message || JSON.stringify(j.error)}`), { http: res.status });
    return { result: j.result, http: res.status };
  }

  async open() {
    await this.rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'chloedb-swarm', version: '0.1.0' } });
    const { result } = await this.rpc('tools/list', {});
    this.tools = result.tools || [];
    return this;
  }

  /** The tools as the Messages API takes them, each with `thought` added. */
  toolDefs() {
    return this.tools.map((t) => {
      const schema = structuredClone(t.inputSchema || { type: 'object', properties: {} });
      schema.type = 'object';
      schema.properties = { thought: { type: 'string', description: 'A short note of what you are doing now, as you would say it aloud.' }, ...(schema.properties || {}) };
      schema.required = ['thought', ...(schema.required || []).filter((r) => r !== 'thought')];
      return { name: t.name, description: (t.description || '').slice(0, 1024), input_schema: schema };
    });
  }

  has(name) { return this.tools.some((t) => t.name === name); }

  /**
   * Call one tool. Answers { text, ok, error?, http } for the step's record
   * and the model; never throws for the tool's own failure, which is the
   * kind of thing the swarm is there to see.
   */
  async call(name, input) {
    const { thought, ...args } = input || {};
    try {
      const { result, http } = await this.rpc('tools/call', { name, arguments: args });
      const text = (result.content || []).map((c) => (c.type === 'text' ? c.text : `[${c.type}]`)).join('\n');
      const cut = text.length > MAX_RESULT ? `${text.slice(0, MAX_RESULT)}\n… (${text.length - MAX_RESULT} more characters cut)` : text;
      return { text: cut, ok: !result.isError, error: result.isError ? text.slice(0, 500) : null, http };
    } catch (e) {
      return { text: `Error: ${e.message}`, ok: false, error: e.message, http: e.http || 0 };
    }
  }

  /** What the person sees at the start. */
  async start() {
    return `You are connected to a ChloeDB server through its MCP tools (${this.tools.map((t) => t.name).join(', ')}).`;
  }

  async close() {}
}
