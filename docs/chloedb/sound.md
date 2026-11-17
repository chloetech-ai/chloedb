# chloedb-sound: Sound Explorer on ChloeDB

> **A demo.** chloedb-sound shows what ChloeDB can do; it is not part of ChloeDB's releases or their promises. It is tested on a train of its own (the `Demos` workflow, `.github/workflows/demos.yml`), outside CI's required check, and the command center marks it as a demo ([compatibility](../compatibility.md#demos)).

`chloedb-sound` keeps free sounds in ChloeDB and finds them by how they sound: field recordings, effects and music that anyone may reuse with credit, each with its author, licence and a link back to where it came from. It listens with CLAP (`clap-htsat-unfused`), which puts sounds and sentences in one space of 512 dimensions, run by chloedb-server itself (built with `--features audio`).

- **Search by how it sounds.** "rain on a tin roof", "a crowd cheering far away", "a cello played softly": the words are heard through CLAP's text tower and ranked against every sound's vector, fused with BM25 over its title, tags, labels and description (`0.75 * tmm(similarity(emb, :v)) + 0.25 * tmm(bm25(words, :q))`). *Rank by* picks one of the two. Facets count the source, the kind, the licence and the length of the best 200.
- **Steer it.** *More like this* and *Less like this* on any result move the search towards and away from that sound's vector; *not* takes words to move away from ("no thunder"); the chips (heavier, lighter, drier, wetter, closer, farther, busier, calmer) move it between two sentences. Each is shown as a chip of what was understood.
- **Moments inside long recordings.** Every sound is cut into ten-second windows every five seconds, each with its own vector (`au_moments`), so a search can land 7:40 into a half-hour dawn chorus. *Moments* returns the best moment of each sound (`DISTINCT ON (sound)`) and the other good moments of each.
- **What is in it.** A sound's page shows, along its waveform, which of FSD50K's 200 labels each moment sounds like (each moment's vector against the labels', `au_labels`, embedded by the server from a sentence each).
- **Listen.** Record ten seconds with the microphone or drop a file, and find what it sounds like: the clip is embedded by `POST /v1/embed` and searched like any sound; a match above 0.9 is called the same recording.
- **Scenes.** Describe a place ("a rainy café by a harbour") and get its layers: the sounds nearest the words and each part of them, one for each thing they sound like most (their best label by sound), played together with a level for each.
- **Drift.** From one sound to another in steps, each step the sound nearest to a point on the way between their vectors (a slerp).
- **Map.** Every sound placed by how it sounds (t-SNE of the vectors, made by `tools/prepare.py`), or by where it was recorded when its source says.
- **Credit and licences.** Each sound carries its title, author, source page and licence, shown on the page and copied as one line ("“Title” by author, source (page), licence (link)"). Only CC0, public domain, CC BY, CC BY-SA and the Free Art License (and the GFDL on Commons) are kept: no non-commercial, no-derivatives or Sampling+ sound is loaded.

Like [the crawler](crawler.md) and [Hacker News](hn.md), it is a program of its own, in Rust, that talks to chloedb-server over HTTP only, and the command center shows it as an app (**Apps → Sound Explorer**). Previews are MP3 files on the app's disk, served by it with HTTP ranges, since a cluster's coordinator keeps no media files.

## Run it

Build the server with CLAP and the app, download the sounds, make the rows, and start the app with a server of its own:

```bash
(cd chloedb-server && cargo build --release --features audio)
cd chloedb-sound && cargo build --release
python3 -I tools/fetch.py fsd50k ~/sounds/fsd50k --clips 2500
python3 -I tools/fetch.py fma ~/sounds/fma --shards 9
python3 -I tools/fetch.py commons ~/sounds/commons --files 300 --min-seconds 20
target/release/chloedb-sound serve --local ./sound-data --server-bin ../chloedb-server/target/release/chloedb-server &
python3 -I tools/prepare.py --fsd50k ~/sounds/fsd50k --fma ~/sounds/fma --commons ~/sounds/commons --out ./sound-rows
target/release/chloedb-sound load --rows ./sound-rows
```

`serve --local DIR` starts a chloedb-server with its database in `DIR`, makes the tables and the labels (the server fetches CLAP, 600 MB, the first time a field names it), and lists the app as `sound`; open `http://127.0.0.1:7700` and go to **Apps → Sound Explorer**. `tools/prepare.py` decodes each sound with ffmpeg, writes its preview and waveform, sends each ten-second window to the server's CLAP (`POST /v1/embed` through `au_labels`' model, about two and a half windows a second on four cores), and writes the rows; it needs ffmpeg and numpy, and a run that stops goes on where it was. Give `serve` the rows with `--rows` and it loads them when the tables are empty, and plays the previews from their `audio/` folder (`--audio` for another).

The three sources, and what is kept of each:

| source | what | licences kept |
|---|---|---|
| [FSD50K](https://zenodo.org/records/4060432) | Freesound clips labelled by hand with 200 classes of the AudioSet ontology; the evaluation set's, the same number of each class | CC0, CC BY |
| [Free Music Archive](https://github.com/mdeff/fma) | 30-second clips of `fma_small`, with their genres | CC BY, CC BY-SA, CC0, Free Art License (about 15% of its tracks) |
| [Wikimedia Commons](https://commons.wikimedia.org/wiki/Category:Sounds_by_type) | sounds under its sound categories, with descriptions and, for some, a place | CC0, public domain, CC BY, CC BY-SA, GFDL |

Freesound's own archive is not used: its terms do not allow a copy of it on another site.

Against a server or a cluster that runs elsewhere, load into it and give the server the app:

```bash
chloedb-sound init --strata http://coordinator:7700 --key "$CHLOEDB_KEY"
python3 -I tools/prepare.py ... --out ./sound-rows --strata http://coordinator:7700 --key "$CHLOEDB_KEY"
chloedb-sound load --rows ./sound-rows --strata http://coordinator:7700 --key "$CHLOEDB_KEY"
chloedb-sound serve --strata http://coordinator:7700 --key "$CHLOEDB_KEY" --token "$SOUND_TOKEN" --audio ./sound-rows/audio --listen 0.0.0.0:7870
chloedb-server ... --app sound=http://sound-host:7870     # with CHLOEDB_APP_TOKEN=$SOUND_TOKEN
```

On Kubernetes, [`deploy/k8s.yaml`](../../chloedb-sound/deploy/k8s.yaml) runs it on one small node of its own (2 vCPUs, 4 GB): the server with CLAP, the app and a pod to work in, with the binaries, the model, the rows, the previews and the database on the node's own disk.

`--public` serves the page at `/` and its reads to anyone, without the token, within `--per-minute` requests a minute from one address (120 by default; the previews are not counted) and `--listens-per-minute` clips heard (6), and refuses calls from another site's pages. It serves no SQL: the page's *SQL* drawer shows the statements each answer ran.

A server without CLAP (built without the `audio` feature, or without the model) still serves the app: searches by words are ranked by BM25 alone and say so, and the pages that hear words or clips answer with an error.

## Tables

| table | a row is | its vector |
|---|---|---|
| `au_sounds` | a sound: title, words (title, tags, labels, description), author and its page, source page, licence and its link, kind, tags, labels, genres, length, year, place, preview file, waveform (160 peaks), place on the map, number of moments | `emb VECTOR(512)`: the mean of its windows that start every ten seconds, the vector the server makes of the whole file |
| `au_moments` | ten seconds of a sound, every five seconds (`shard_by = 'sound'`, so a sound's moments are on one shard) | `emb VECTOR(512)` |
| `au_labels` | one of FSD50K's 200 labels, with a sentence ("the sound of rain") | `emb VECTOR(512) OPTIONS(embed = 'prompt', model = 'clap-htsat-unfused')`: the server embeds it, and words and clips are embedded through this field |

## The API

Every answer carries `queries`, the SQL it ran, for the page's *SQL* drawer.

| route | what |
|---|---|
| `GET /v1/search?q=&like=&unlike=&not=&steer=&source=&kind=&lic=&min_s=&max_s=&by=&page=` | sounds by words and by sound, steered, filtered and faceted; `by` is `both`, `sound` or `words` |
| `GET /v1/sound/:id` | a sound with its credit, what is in it moment by moment, sounds like it and more by its author |
| `GET /v1/moments?q=&min=` | the best moment of each recording, and its other good moments, in recordings at least `min` seconds long (15 by default; songs are left to *Music*) |
| `POST /v1/listen` | `{audio: base64}` (WAV, FLAC, MP3 or Ogg, at most 2 MB): what it sounds like, and whether it is one of the sounds |
| `GET /v1/scene?q=` | a scene's layers: a sound for each thing the place sounds like, with a level |
| `GET /v1/drift?from=&to=&steps=` | the sounds on the way from one to the other |
| `GET /v1/map` | every sound's place on the map and by where it was recorded |
| `GET /v1/home`, `/v1/labels`, `/v1/status` | the front page, the labels, the counts |
| `GET /v1/audio/:id` | a preview (MP3), with ranges; `?b64=1` as JSON for the command center |

## Tests

`cargo test` runs the app against a real chloedb-server (`CHLOEDB_SERVER_BIN`, or one built in `../chloedb-server`) with nine made-up sounds and their moments: search by sound, steering, filters, words, a sound's credit, drift, the map, previews with ranges, and the public limits. `CHLOEDB_SOUND_CLAP=1` against a server built with `--features audio` tests the pages that hear words and clips too; without it the test checks that words are still searched by BM25.
