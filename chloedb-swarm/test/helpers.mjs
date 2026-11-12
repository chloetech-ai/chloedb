// A scripted model and a fake surface, so the loop, the runner and the
// judge can be tested without the API or a browser.

/**
 * A client whose messages.create answers from `script` in order: { tool,
 * input } is a tool call, { text } is words only. It records each request.
 * `pick(params)` may choose the answer instead (for concurrent callers).
 */
export function scripted(script, pick) {
  let i = 0;
  const requests = [];
  const create = async (params) => {
        requests.push(structuredClone(params));
        const s = pick ? pick(params) : script[i++] || { text: 'nothing more' };
        const content = s.tool ? [{ type: 'tool_use', id: `tu_${requests.length}`, name: s.tool, input: s.input }] : [{ type: 'text', text: s.text }];
        return { model: params.model, content, stop_reason: s.tool ? 'tool_use' : 'end_turn', usage: { input_tokens: 1000, output_tokens: 100 } };
  };
  return { requests, messages: { create }, beta: { messages: { create } } };
}

export class FakeSurface {
  constructor(answers = {}) { this.answers = answers; this.calls = []; }
  async open() { return this; }
  toolDefs() {
    const t = (name) => ({ name, description: name, input_schema: { type: 'object', properties: { note: { type: 'string' } }, required: ['note'] } });
    return [t('look'), t('press')];
  }
  async start() { return 'A page with a search box.'; }
  async call(name, input) {
    this.calls.push([name, input]);
    const a = this.answers[name];
    return a ? { text: a.error ? `That did not work: ${a.error}` : 'ok', ...a } : { text: `the page after ${name}`, ok: true, error: null };
  }
  async close() {}
}

export const persona = { id: 'tester', name: 'Maya Test', role: 'a tester', device: 'desktop', traits: ['Patient.'] };
export const task = { id: 'find', title: 'Find it', surface: 'fake', prompt: 'Find the refund ticket.', checks: [] };
