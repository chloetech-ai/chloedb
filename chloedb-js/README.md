# chloedb: ChloeDB from JavaScript

ChloeDB opened from its file inside a Node, Bun or Deno process, with no
server, the way better-sqlite3 opens a SQLite file; or a client of a running
chloedb-server from any JavaScript runtime with `fetch`. Both have the same
methods. Documentation: [docs/chloedb/javascript.md](../docs/chloedb/javascript.md).

```js
import { open } from "chloedb";

const db = await open("books.chloedb", {
  schema: { table: "books", key: "id", fields: [
    { name: "id", type: "keyword" },
    { name: "title", type: "text" },
    { name: "year", type: "int" },
  ] },
});
await db.upsert([{ id: "b1", title: "The river and the dragon", year: 1999 }]);
for (const row of await db.search("dragon")) console.log(row.id, row.title);
await db.close();
```

Prebuilt for macOS and Linux (no Rust needed): `npm install` the tarball of
your platform from a `chloedb-js-v<version>` GitHub release
([docs](../docs/chloedb/javascript.md#prebuilt)); release 0.1.0, from before
the rename, was published as `strata-db`. Build (Rust and a C compiler), test,
and use from an app:

```sh
npm run build                         # cargo build --release; chloedb.node beside index.js
npm run build -- --profile ci && npm test
cd ../my-app && npm install ../chloedb/chloedb-js
```

| file | what |
|---|---|
| `src/lib.rs` | the Node-API addon (napi-rs): one entry point per database, an operation's name and its arguments as JSON, run on libuv's thread pool |
| `index.js`, `index.d.ts` | `Database`, `open`, `fromSqlite`, `fromParquet` and the rest, over the addon |
| `client.js`, `client.d.ts` | `Client` and `connect`: chloedb-server over HTTP with `fetch`, no addon |
| `common.js`, `common.d.ts` | what both share: `Result`, `ChloeDBError` (`StrataError` by its old name), `search`, the rank log, JSON with 64-bit integers |
| `scripts/build.mjs` | builds the addon with cargo and copies it to `chloedb.node` |
| `scripts/pack.mjs` | packs the package with its addon for one platform (`dist/chloedb-<version>-<platform>.tgz`); `.github/workflows/js-release.yml` runs it per platform on a tag |
| `test/` | `node --test`: the database in-process, and the client against a chloedb-server |
