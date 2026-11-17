// MCP Atlas — credential taxonomy and the binding rules.
//
// Everything here states what a server *declares* in its registry record. Nothing
// here judges a server. A binding row's `basis` is one of:
//   exact    — a remote URL and a declared secret header on it: host and position
//              are both in the record.
//   inferred — a curated map from a well-known environment-variable NAME pattern to
//              a provider host; shown with the rule that produced it.
//   unknown  — a declared secret whose destination we cannot name: "unbound: denied
//              by default".
//
// `bucket` is the coarse class used to colour the map (nine colours). `class` is the
// finer provider-level class used for the credential node table and the credential view.

// Coarse buckets -> colour token name (resolved to a hex on the client from ds.css).
export const BUCKETS = {
  none:        { label: 'No declared secret', color: 'faint'  },
  'api-key':   { label: 'Generic API key',    color: 'indigo' },
  llm:         { label: 'LLM provider key',   color: 'violet' },
  vcs:         { label: 'Source host (GitHub/GitLab)', color: 'cyan' },
  cloud:       { label: 'Cloud provider',     color: 'green'  },
  saas:        { label: 'SaaS / productivity', color: 'blue'  },
  send:        { label: 'Messaging / email',  color: 'amber'  },
  pay:         { label: 'Payments',           color: 'teal'   },
  database:    { label: 'Database / store',   color: 'orange' },
  password:    { label: 'Password',           color: 'pink'   },
  'private-key': { label: 'Private key / wallet', color: 'red' },
};

// Priority order decides a server's primary bucket for the map (most sensitive wins).
export const BUCKET_PRIORITY = [
  'private-key', 'password', 'database', 'pay', 'cloud', 'vcs', 'send', 'llm', 'saas', 'api-key', 'none',
];

// Curated provider map: the top environment-variable NAME patterns -> a provider host.
// Order matters (first match wins). Each entry is the rule shown on an inferred row.
// Class names follow the providers catalogued in secrets/provider-formats.jsonl.
const P = (re, cls, host, bucket, kind = 'api-key') => ({ re: new RegExp(re, 'i'), cls, host, bucket, kind });
export const PROVIDERS = [
  // version control
  P('^(gh|github)_(token|pat|personal_access_token|key)$|^github_token$', 'github', 'api.github.com', 'vcs', 'token'),
  P('gitlab_(token|pat|personal_access_token)', 'gitlab', 'gitlab.com', 'vcs', 'token'),
  P('bitbucket_(token|app_password|key)', 'bitbucket', 'api.bitbucket.org', 'vcs', 'token'),
  // llm providers
  P('openai_(api_)?key', 'openai', 'api.openai.com', 'llm', 'key'),
  P('anthropic_(api_)?key', 'anthropic', 'api.anthropic.com', 'llm', 'key'),
  P('(gemini|google_genai|google_ai|googleai)_api_key', 'google-gemini', 'generativelanguage.googleapis.com', 'llm', 'key'),
  P('mistral_api_key', 'mistral', 'api.mistral.ai', 'llm', 'key'),
  P('cohere_api_key', 'cohere', 'api.cohere.ai', 'llm', 'key'),
  P('groq_api_key', 'groq', 'api.groq.com', 'llm', 'key'),
  P('together_api_key', 'together', 'api.together.xyz', 'llm', 'key'),
  P('perplexity_api_key|pplx_api_key', 'perplexity', 'api.perplexity.ai', 'llm', 'key'),
  P('(hf_token|huggingface(hub)?_(api_)?(token|key))', 'huggingface', 'huggingface.co', 'llm', 'token'),
  P('replicate_api_token', 'replicate', 'api.replicate.com', 'llm', 'token'),
  P('openrouter_api_key', 'openrouter', 'openrouter.ai', 'llm', 'key'),
  P('deepseek_api_key', 'deepseek', 'api.deepseek.com', 'llm', 'key'),
  P('xai_api_key|grok_api_key', 'xai', 'api.x.ai', 'llm', 'key'),
  // cloud
  P('aws_secret_access_key|aws_access_key_id|aws_session_token', 'aws', 'amazonaws.com', 'cloud', 'key'),
  P('google_application_credentials|gcp_|gcloud_', 'gcp', 'googleapis.com', 'cloud', 'key'),
  P('azure_(client_secret|api_key|openai_api_key)', 'azure', 'azure.com', 'cloud', 'key'),
  P('cloudflare_api_token|cf_api_token', 'cloudflare', 'api.cloudflare.com', 'cloud', 'token'),
  P('digitalocean_(token|api_token)|do_api_token', 'digitalocean', 'api.digitalocean.com', 'cloud', 'token'),
  // saas / productivity
  P('slack_(bot_)?(token|api_token)', 'slack', 'slack.com', 'saas', 'token'),
  P('notion_(api_)?(key|token)', 'notion', 'api.notion.com', 'saas', 'token'),
  P('linear_api_key', 'linear', 'api.linear.app', 'saas', 'key'),
  P('(atlassian|jira|confluence)_(api_token|token)', 'atlassian', 'api.atlassian.com', 'saas', 'token'),
  P('figma_(api_)?(token|key)', 'figma', 'api.figma.com', 'saas', 'token'),
  P('airtable_(api_)?key', 'airtable', 'api.airtable.com', 'saas', 'key'),
  P('hubspot_(api_)?(key|token)', 'hubspot', 'api.hubapi.com', 'saas', 'token'),
  P('intercom_(access_)?token', 'intercom', 'api.intercom.io', 'saas', 'token'),
  P('asana_(access_)?token', 'asana', 'app.asana.com', 'saas', 'token'),
  // search / data
  P('brave_(api_)?key|brave_search', 'brave', 'api.search.brave.com', 'saas', 'key'),
  P('tavily_api_key', 'tavily', 'api.tavily.com', 'saas', 'key'),
  P('(serpapi|serp_api)_key', 'serpapi', 'serpapi.com', 'saas', 'key'),
  P('exa_api_key', 'exa', 'api.exa.ai', 'saas', 'key'),
  P('firecrawl_api_key', 'firecrawl', 'api.firecrawl.dev', 'saas', 'key'),
  P('pinecone_api_key', 'pinecone', 'pinecone.io', 'database', 'key'),
  // payments
  P('stripe_(secret_|api_)?key|stripe_sk', 'stripe', 'api.stripe.com', 'pay', 'key'),
  P('paypal_(client_secret|secret)', 'paypal', 'api.paypal.com', 'pay', 'secret'),
  P('coinbase_(api_)?(key|secret)', 'coinbase', 'api.coinbase.com', 'pay', 'key'),
  // messaging / email
  P('sendgrid_api_key', 'sendgrid', 'api.sendgrid.com', 'send', 'key'),
  P('(mailgun)_api_key', 'mailgun', 'api.mailgun.net', 'send', 'key'),
  P('resend_api_key', 'resend', 'api.resend.com', 'send', 'key'),
  P('postmark_(server_)?token', 'postmark', 'api.postmarkapp.com', 'send', 'token'),
  P('twilio_(auth_token|api_key)', 'twilio', 'api.twilio.com', 'send', 'token'),
  P('telegram_bot_token', 'telegram', 'api.telegram.org', 'send', 'token'),
  P('discord_(bot_)?token', 'discord', 'discord.com', 'send', 'token'),
  // databases / stores
  P('supabase_(service_role_key|key|anon_key|url)', 'supabase', 'supabase.co', 'database', 'key'),
  P('redis_(url|password)', 'redis', 'redis', 'database', 'url'),
  P('(mongo_url|mongodb_uri|mongodb_url)', 'mongodb', 'mongodb', 'database', 'url'),
  P('(database_url|db_url|database_uri|db_uri|postgres_url|postgresql_url|mysql_url|connection_string|conn_string|datasource_url|dsn|jdbc_url)', 'database', 'database', 'database', 'url'),
];

// Secrets catalogued by name but with no public destination host.
const PRIVATE_KEY_RE = /(private[_-]?key|seed[_-]?phrase|mnemonic|wallet)/i;
const PASSWORD_RE = /password/i;
// A name that "looks like" a secret: one of four words.
export const KEYISH_RE = /(key|token|secret|password)/i;

// Classify one declared environment variable. Returns a binding descriptor or null.
export function classifyEnvVar(name = '', desc = '', isSecret = false) {
  const nm = String(name || '');
  // private key / wallet first — most sensitive, no host.
  if (PRIVATE_KEY_RE.test(nm)) {
    return { cls: 'private-key', host: null, bucket: 'private-key', kind: 'private-key', basis: 'unknown', method: `env:${nm}` };
  }
  for (const p of PROVIDERS) {
    if (p.re.test(nm)) {
      return { cls: p.cls, host: p.host, bucket: p.bucket, kind: p.kind, basis: 'inferred',
               method: `env:${nm}`, rule: p.re.source };
    }
  }
  if (PASSWORD_RE.test(nm)) {
    return { cls: 'password', host: null, bucket: 'password', kind: 'password', basis: 'unknown', method: `env:${nm}` };
  }
  // declared secret with no known destination
  if (isSecret || KEYISH_RE.test(nm)) {
    return { cls: 'api-key', host: null, bucket: 'api-key', kind: 'key', basis: 'unknown', method: `env:${nm}` };
  }
  return null;
}

// Classify a remote's declared secret header. host is the remote's host -> an exact binding.
export function classifyHeader(headerName = '', host = '') {
  const h = String(host || '').toLowerCase();
  let cls = 'bearer', bucket = 'api-key';
  for (const p of PROVIDERS) {
    if (p.host && h.endsWith(p.host.replace(/^api\./, ''))) { cls = p.cls; bucket = p.bucket; break; }
  }
  return { cls, host: h || null, bucket, kind: 'header', basis: 'exact', method: `header:${headerName}` };
}

export function pickPrimaryBucket(buckets) {
  const set = new Set(buckets);
  for (const b of BUCKET_PRIORITY) if (set.has(b)) return b;
  return 'none';
}
