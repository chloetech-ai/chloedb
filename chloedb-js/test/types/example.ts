// A small typed app of the package, which tsc checks without Node's types
// (test/types.test.mjs): rows typed by `query<T>()`, a write's count, a
// cursor, highlighting and "did you mean", in the process and over HTTP.
import { open, type Database, type Field, type Schema } from "../../index.js";
import { connect, type Client } from "../../client.js";

interface Product {
  id: string;
  name: string;
  price: number;
}

const fields: Field[] = [
  { name: "id", type: "keyword" },
  { name: "name", type: "TEXT" },
  { name: "price", type: "float" },
];
// @ts-expect-error: a field's type is one of the types a schema takes
const misspelt: Field = { name: "brand", type: "keywrd" };
export const schema: Schema = { table: "products", key: "id", fields: [...fields, misspelt] };

async function page(db: Database | Client, typed: string): Promise<{ hits: (Product & { marked: string })[]; didYouMean: string | null; next: string | null }> {
  const hits = await db.query<Product>("SELECT id, name, price FROM products WHERE MATCH(name, :q) LIMIT 20", { q: typed }, { pageSize: 10 });
  const cheapest: number = Math.min(...hits.objects().map((p) => p.price));
  const marked: string[] = await db.highlight(typed, hits.column("name") as string[], { field: "name" });
  const one: string = await db.highlight(typed, "Leather sofa");
  const fixed = await db.correct(typed, { field: "name" });
  const shown: string[] = fixed.words.map((w) => `${w.word} -> ${w.toWord} (${w.edits}, ${w.chance.toFixed(2)})`);
  void cheapest, one, shown;
  return { hits: hits.objects().map((p, i) => ({ ...p, marked: marked[i] ?? p.name })), didYouMean: fixed.changed ? fixed.text : null, next: hits.cursor };
}

export async function main(): Promise<void> {
  await using db = await open(":memory:", { schema, maxResultWindow: 10_000 });
  const n: number = await db.execute("DELETE FROM products WHERE price < :p", { p: 1 });
  const p: Product | null = await db.get<Product>("p1");
  const first = await page(db, "lether so*");
  if (first.next) {
    const more = await db.next<Product>(first.next, { keepAliveMs: 30_000 });
    for (const row of more) row.name.toUpperCase();
    if (more.cursor) await db.closeCursor(more.cursor);
  }
  for await (const rows of db.scroll<Product>("SELECT id, name, price FROM products", null, { pageSize: 500 })) {
    rows.objects().forEach((r) => r.price.toFixed(2));
  }
  const either = await db.sql<Product>("SELECT id, name, price FROM products");
  if (typeof either !== "number") either.at(0)?.id.toLowerCase();

  await using client = connect("http://127.0.0.1:7700");
  const written: number = await client.execute("UPDATE products SET price = price * 0.9", null, { idempotencyKey: "sale-1" });
  await page(client, "sofa");
  for await (const rows of client.scroll<Product>("SELECT id, name, price FROM products", null, { pageSize: 1000, minSeqNo: 1 })) rows.length;
  void n, p, written;
}
