# chloedb-img: pictures searched by what is in them

> **A demo.** chloedb-img shows what ChloeDB can do; it is not part of ChloeDB's releases or their promises. It is tested on a train of its own (the `Demos` workflow, `.github/workflows/demos.yml`), outside CI's required check, and the command center marks it as a demo ([compatibility](../compatibility.md#demos)).

`chloedb-img` is an explorer of [PD12M](https://huggingface.co/datasets/Spawning/PD12M): 12.4 million pictures that are in the public domain or under CC0, from museums, libraries, archives and Wikimedia Commons, each with a caption written by a model and the vector OpenAI's CLIP (ViT-L/14) gives it. It is a program of its own, in Rust, that talks to chloedb-server over HTTP, like [the arXiv explorer](arxiv.md), and the command center shows it as an app. It embeds words and photos itself, with ChloeDB's CLIP (`clip-vit-large-patch14`, feature `vision`), so the server it searches is the stock build, with no model.

Every search is one statement over the pictures' table, `ORDER BY similarity(clip, :v)` on its HNSW index, with the filters a `WHERE`:

- **Search by what is in a picture.** *A lighthouse in a storm*, *botanical drawing of a fern*, *children on a beach in the 1920s*: the words are embedded by CLIP's text tower and meet the pictures' vectors. Kind (photo, old photo, painting, print, drawing, map, text page, object, illustration), source, licence, shape and size are facets of the closest 2,000, and filters of the query.
- **Search with a photo of your own.** It is embedded and thrown away; nothing is kept. The answer is the closest pictures, the closest photos and the closest paintings, prints and drawings.
- **Search by a detail.** Draw a box on a picture and get the pictures that look like what is in it.
- **More like this, less like that.** Mark pictures up or down and the query moves toward and away from them (Rocchio); *strongly* moves it twice as far.
- **A picture plus words.** *This harbour, but at night*: the picture's vector and the words' added with a weight the reader slides.
- **Between two pictures.** The path from one picture to another in steps along the sphere between their vectors (slerp), the closest picture at each step.
- **More variety.** Results re-ranked so that near-copies do not fill a page (maximal marginal relevance).
- **Another medium.** A picture's subject as photos, paintings, prints or drawings: its vector plus its subject in words, kept to that medium's kinds.
- **Colors.** Pick up to three colors; each picture whose thumbnail has been made has a histogram of its colors in Lab (144 bins, `img_colors`), and a color search is a vector search on it. The page says how many pictures have colors so far.
- **A map of themes.** 42 themes found by k-means over the vectors (ships and harbours, botanical plates, medieval manuscripts, old maps and views, moths, ...), placed so that near themes are near, each with the pictures nearest its middle; a theme's page is its pictures in order of closeness to its centre.
- **Every picture's page** shows it large, its licence and source with a link to the original and, for a picture from Wikimedia Commons, its page there (found by the file's SHA-1, since PD12M's records do not give it), with the pictures nearest it.

## Where the data comes from

- **Pictures**: PD12M's records and CLIP vectors (Parquet and NumPy files on Hugging Face), and the images themselves from PD12M's public bucket on S3. Thumbnails are made the first time a picture is shown and kept on disk: one 400 pixels high for the grids, and one up to 1,600 pixels for a picture's page, made only when that page is opened. The colors and the Commons page are found in the same pass.
- **Kinds and themes**: `prepare` names each picture's kind by the closest of a few CLIP sentences (*a photograph*, *an old black and white photograph*, *an oil painting*, ...) and its theme by k-means of a sample of the vectors (spherical, seeded by k-means++ with a fixed seed, so the same slice gets the same themes). Each theme is named by the closest of a list of subjects, or by `--names` (`data/theme-names-1m.json` names those of the first million). `--themes-from` keeps the themes of an earlier prepare (their centres, names and places on the map) and places every picture in the nearest, so a larger slice keeps a smaller one's map.
- **Licences**: every picture is shown with its licence as PD12M gives it (CC0 or the Public Domain Mark) and its source, linked back to where it came from.

## Run it

PD12M's first million pictures need the first ten Parquet files and the first vector file; CLIP's weights (1.7 GB) come from `chloedb-server fetch-model clip-vit-large-patch14`.

```bash
chloedb-img prepare --data ~/pd12m --rows 1000000 --models ~/.local/share/chloedb/models --out ~/img-prepared --names chloedb-img/data/theme-names-1m.json
chloedb-img serve --local ~/img-db --models ~/.local/share/chloedb/models --cache ~/img-cache   # starts a chloedb-server with img as its app
chloedb-img load --data ~/pd12m --prepared ~/img-prepared --rows 1000000                       # about three minutes for a million
```

With `--public` the app serves the page at `/` to anyone, within `--per-minute` requests, `--photos-per-minute` photos and details and `--thumbs-per-minute` new thumbnails a minute from one address. There is no SQL from the page and no MCP tools. `deploy/k8s.yaml` runs it on Kubernetes: one 16 GB node with the server and the app, the database and the thumbnails on the node's own SSD. `--warm-per-theme` makes the thumbnails of the pictures nearest each theme's middle at start, and `--warm-first N` those of the first N pictures, so that colors cover more of them.

## The tables

| table | what |
|---|---|
| `img_pictures` | a picture: its file, title (the caption's first clause), caption, source, licence, size, shape, kind, theme, and `clip`, its CLIP vector (HNSW) |
| `img_themes` | a theme: its name, size, place on the map, the pictures nearest its middle, the themes nearest it and its centre |
| `img_colors` | the colors of each picture whose thumbnail was made: its palette and a 144-bin Lab histogram as a vector |
| `img_links` | a picture's page on Wikimedia Commons, when one was found |

## Tests

```bash
cd chloedb-img && cargo test --profile ci
```

They cover titles from captions, licences, shapes and sizes, colors in Lab and color search, k-means and theme names, filters as parameters, and which routes are public. Searching needs CLIP's weights and PD12M, so CI runs no server for it.
