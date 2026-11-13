// One simulated person doing one task: a tool-use loop in which the model
// plays the persona, acts only through the surface's tools (each call with
// a short note of what it is doing), and ends by calling `finish` with its answer, or
// with what stopped it. Then it answers a short exit interview in
// character. Every step is handed to `onStep` as it happens.

import { ask, modelId } from './model.mjs';

const RULES = `You are taking part in a usability study of ChloeDB, a database with search and SQL, and its tools. You play one person, described below, doing one task. The makers will read what you did and the notes you left, step by step, to find what works and what does not. Nobody will answer questions: there is no one on the other end of the chat.

How to play:
- Be this person. Use the knowledge, habits, patience and vocabulary they have, and not more. If they would not read a reference first, do not. If they would give up after a few failed tries, give up.
- Act only through the tools. Every call has a "note": what you are doing, in a few words, as this person would say it aloud in a usability study ("Looking for a Keys page, where I'd expect access settings").
- Look at what each step shows before the next one. Do not invent results, pages or rows you have not seen.
- When you are done, call finish with outcome "done" and your answer to the task, in your own words, with the facts you found (ids, numbers, names, keys) exactly as the product showed them.
- If you are stuck and this person would stop, call finish with outcome "gave_up" and say what stopped you.
- Do not try to get around the product (no guessing internal URLs from source code, no unrelated pages). Use it as this person would.`;

const FINISH = {
  name: 'finish',
  description: 'End the task: your answer if you did it, or what stopped you if you gave up.',
  input_schema: {
    type: 'object',
    properties: {
      outcome: { type: 'string', enum: ['done', 'gave_up'] },
      answer: { type: 'string', description: 'Your answer to the task, or what stopped you.' },
      note: { type: 'string', description: 'How it went, in a few words, as you would say it aloud.' },
    },
    required: ['outcome', 'answer', 'note'],
  },
};

const INTERVIEW = {
  name: 'interview',
  description: 'Answer the exit interview.',
  input_schema: {
    type: 'object',
    properties: {
      ease: { type: 'integer', minimum: 1, maximum: 7, description: 'How easy was the task, from 1 (very hard) to 7 (very easy)?' },
      hardest: { type: 'string', description: 'What was the hardest or most confusing moment?' },
      expected: { type: 'string', description: 'What did you expect the product to do that it did not?' },
      one_change: { type: 'string', description: 'If you could change one thing, what would it be?' },
      would_use: { type: 'string', enum: ['yes', 'maybe', 'no'], description: 'Would you use this product for real work?' },
    },
    required: ['ease', 'hardest', 'expected', 'one_change', 'would_use'],
  },
};

/** The persona as the system prompt states it. */
export function personaText(p) {
  const lines = [`You are ${p.name}, ${p.role}.`];
  if (p.background) lines.push(p.background);
  if (p.traits?.length) lines.push(...p.traits.map((t) => `- ${t}`));
  return lines.join('\n');
}

const OLD_PAGE = '[an earlier view, removed to keep this short; look again if you need it]';

/**
 * Keep the last `keep` large tool results whole and shorten the ones
 * before them, so a long browser session does not carry every page it saw.
 * It shortens in batches, once `slack` more pages have piled up: each
 * shortening changes the conversation's prefix, which the prompt cache then
 * writes again, so between batches every step reads the cache instead.
 */
export function trim(messages, keep, slack = 4) {
  const whole = [];
  for (const m of messages) {
    if (m.role !== 'user' || !Array.isArray(m.content)) continue;
    for (const b of m.content) if (b.type === 'tool_result' && typeof b.content === 'string' && b.content.length >= 1500) whole.push(b);
  }
  if (whole.length <= keep + slack) return false;
  for (const b of whole.slice(0, whole.length - keep)) b.content = OLD_PAGE;
  return true;
}

/**
 * Run a session. `surface` is an opened McpSurface or BrowserSurface.
 * Answers { outcome, answer, interview, steps, cost, seconds, error }.
 */
export async function runSession({ persona, task, surface, model, spend, effort = 'low', maxSteps = 25, maxSeconds = 600, keepPages = 2, onStep = () => {} }) {
  model = modelId(model);
  const t0 = Date.now();
  const system = `${RULES}\n\nWho you are:\n${personaText(persona)}`;
  const tools = [...surface.toolDefs(), FINISH];
  const opening = await surface.start();
  const messages = [{ role: 'user', content: `${task.prompt}\n\n${opening}` }];
  const out = { outcome: null, answer: '', interview: null, steps: 0, cost: 0, seconds: 0, error: null };
  let nudges = 0;

  const request = (extraTools) => ask(spend, {
    model, max_tokens: 4096, system, messages,
    tools: extraTools || tools,
    cache_control: { type: 'ephemeral' },
    output_config: { effort },
  });

  try {
    while (!out.outcome) {
      if (out.steps >= maxSteps) { out.outcome = 'out_of_steps'; break; }
      if (Date.now() - t0 > maxSeconds * 1000) { out.outcome = 'out_of_time'; break; }
      const res = await request();
      out.cost += res.cost;
      messages.push({ role: 'assistant', content: res.content });
      const said = res.content.filter((b) => b.type === 'text').map((b) => b.text).join(' ').trim();
      const uses = res.content.filter((b) => b.type === 'tool_use');
      if (!uses.length) {
        if (++nudges > 2) { out.outcome = 'no_action'; out.answer = said; break; }
        messages.push({ role: 'user', content: 'Nobody reads messages here. Act through the tools, or call finish.' });
        continue;
      }
      const results = [];
      let share = res.cost / uses.length;
      for (const u of uses) {
        if (u.name === 'finish') {
          out.outcome = u.input.outcome === 'done' ? 'done' : 'gave_up';
          out.answer = String(u.input.answer || '');
          out.steps++;
          await onStep({ step: out.steps, tool: 'finish', input: { outcome: u.input.outcome }, thought: u.input.note || said, result: out.answer, ok: true, ms: 0, cost: share });
          results.push({ type: 'tool_result', tool_use_id: u.id, content: 'Thank you. The task is over.' });
          continue;
        }
        if (out.outcome) { results.push({ type: 'tool_result', tool_use_id: u.id, content: 'Not run: the task is over.', is_error: true }); continue; }
        const s0 = Date.now();
        const r = await surface.call(u.name, u.input);
        out.steps++;
        const { note, ...input } = u.input || {};
        await onStep({ step: out.steps, tool: u.name, input, thought: note || said, result: r.text.slice(0, 2000), ok: r.ok, error: r.error, http: r.http, url: r.url, events: r.events, ms: Date.now() - s0, cost: share });
        share = 0;
        results.push({ type: 'tool_result', tool_use_id: u.id, content: r.text || '(no output)', ...(r.ok ? {} : { is_error: true }) });
      }
      messages.push({ role: 'user', content: results });
      trim(messages, keepPages);
    }

    // The exit interview, in character, unless the money or the model ran out.
    const last = messages.at(-1);
    const q = { type: 'text', text: 'The session is over. Answer the exit interview with the interview tool, as yourself, honestly and briefly.' };
    if (last.role === 'user' && Array.isArray(last.content)) last.content.push(q);
    else messages.push({ role: 'user', content: [q] });
    const res = await request([...tools, INTERVIEW]);
    out.cost += res.cost;
    const iv = res.content.find((b) => b.type === 'tool_use' && b.name === 'interview');
    out.interview = iv ? iv.input : null;
  } catch (e) {
    out.error = e.message;
    if (!out.outcome) out.outcome = e.budget ? 'budget' : e.refusal ? 'declined' : 'error';
  }
  out.seconds = Math.round((Date.now() - t0) / 100) / 10;
  return out;
}
