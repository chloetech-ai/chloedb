// Column types: what a table's column may be, and its type in Strata.
export const TYPES = {
  text: 'TEXT',            // prose people search by words
  keyword: 'KEYWORD',      // short labels: status, owner, category, email; filter, group and sort by them
  'keyword[]': 'KEYWORD[]',// tags
  int: 'INT',
  float: 'FLOAT',
  bool: 'BOOL',
  timestamp: 'TIMESTAMP',  // ISO 8601 text in and out
  meaning: 'VECTOR',       // what other text columns mean, embedded by Strata as rows are written
};

// The embedding model of meaning columns: Strata computes each row's vector
// when it is written (and fills in rows written before the column).
export const EMBED_MODEL = process.env.STRATA_APPS_EMBED_MODEL || 'potion-retrieval-32M';

// A column as SQL: "name TEXT", or a meaning column's vector of other columns.
export const colSql = (c) => (c.type === 'meaning'
  ? `${c.name} VECTOR OPTIONS(embed = '${(Array.isArray(c.of) ? c.of : [c.of]).filter(Boolean).join(', ')}', model = '${EMBED_MODEL}')`
  : `${c.name} ${TYPES[c.type]}`);
