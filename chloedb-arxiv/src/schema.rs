//! The app's tables. Every one is an ordinary ChloeDB table that any SQL
//! reads: the papers with their text indexed for words and for meaning,
//! the people, the topics, the techniques and domains, two graphs as edge
//! tables (who cites whom, who writes with whom), the events a recommender
//! learns from, and what readers keep: their feeds, issues and library.

use serde_json::Value;

use crate::strata::{Result, Strata};

/// The embedding model of titles and abstracts: a static model, tens of
/// microseconds a text on one core, so every paper embeds on a CPU.
pub const MODEL: &str = "potion-retrieval-32M";

/// The recommender of what to read next (`CREATE RECOMMENDER`).
pub const RECOMMENDER: &str = "reading";

pub fn tables(model: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ax_authors", "CREATE TABLE IF NOT EXISTS ax_authors (
            id KEYWORD PRIMARY KEY,
            name TEXT OPTIONS(analyzer = 'simple'),
            papers INT, first_author INT, cites INT, h INT,
            first TIMESTAMP, last TIMESTAMP, recent INT,
            cats KEYWORD[], techniques KEYWORD[], coauthors INT
        )".to_string()),
        ("ax_papers", format!("CREATE TABLE IF NOT EXISTS ax_papers (
            id KEYWORD PRIMARY KEY,
            title TEXT OPTIONS(substring = true),
            abstract TEXT,
            authors TEXT OPTIONS(analyzer = 'simple'),
            people KEYWORD[], first_author KEYWORD, nauthors INT,
            cats KEYWORD[], pcat KEYWORD, area KEYWORD, areas KEYWORD[],
            created TIMESTAMP, updated TIMESTAMP, year INT, month KEYWORD, versions INT,
            doi KEYWORD, journal TEXT OPTIONS(indexed = false), venue KEYWORD, venue_year INT,
            comments TEXT OPTIONS(indexed = false), pages INT, figures INT,
            code KEYWORD, hf KEYWORD,
            techniques KEYWORD[], domains KEYWORD[], kinds KEYWORD[],
            cites INT, pop FLOAT, refs INT, cited_in INT,
            license KEYWORD, topic INT,
            emb VECTOR OPTIONS(embed = 'title, abstract', model = '{model}')
        )")),
        // The graphs' nodes on the graph server: every paper's id and every
        // person's key, the rows a walk (`ppr`) ranks.
        ("ax_nodes", "CREATE TABLE IF NOT EXISTS ax_nodes (
            id KEYWORD PRIMARY KEY
        )".to_string()),
        // Who cites whom between arXiv papers (a citing work that is a
        // paper's published version, by its DOI), for graph walks.
        ("ax_cites", "CREATE TABLE IF NOT EXISTS ax_cites (
            id KEYWORD PRIMARY KEY,
            src KEYWORD NOT NULL REFERENCES ax_nodes,
            dst KEYWORD NOT NULL REFERENCES ax_nodes
        ) OPTIONS(edges = 'src, dst')".to_string()),
        // Who wrote two papers or more together, weighted by how many.
        ("ax_coauthors", "CREATE TABLE IF NOT EXISTS ax_coauthors (
            id KEYWORD PRIMARY KEY,
            src KEYWORD NOT NULL REFERENCES ax_nodes,
            dst KEYWORD NOT NULL REFERENCES ax_nodes,
            w FLOAT
        ) OPTIONS(edges = 'src, dst, w')".to_string()),
        // What a reader read: the papers each citing work cites (a
        // reader's list, oldest first), and the papers people open and
        // keep here. The recommender counts them as they are written.
        ("ax_events", "CREATE TABLE IF NOT EXISTS ax_events (
            id KEYWORD PRIMARY KEY,
            usr KEYWORD, item KEYWORD, ts TIMESTAMP, kind KEYWORD
        )".to_string()),
        ("ax_topics", "CREATE TABLE IF NOT EXISTS ax_topics (
            id INT PRIMARY KEY,
            label TEXT, about TEXT, terms KEYWORD[], field INT, field_label KEYWORD,
            papers INT, recent INT, growth FLOAT, cites INT, first INT, peak INT,
            by_year TEXT OPTIONS(indexed = false), cats KEYWORD[], techniques KEYWORD[], domains KEYWORD[],
            x FLOAT, y FLOAT, centroid VECTOR(512)
        )".to_string()),
        ("ax_concepts", "CREATE TABLE IF NOT EXISTS ax_concepts (
            id KEYWORD PRIMARY KEY,
            kind KEYWORD, name TEXT, phrases TEXT, acronyms TEXT OPTIONS(analyzer = 'simple'),
            papers INT, cites INT, recent INT, growth FLOAT, first INT,
            by_year TEXT OPTIONS(indexed = false), cats KEYWORD[], related KEYWORD[]
        )".to_string()),
        ("ax_cats", "CREATE TABLE IF NOT EXISTS ax_cats (
            id KEYWORD PRIMARY KEY,
            name TEXT, area KEYWORD, area_name KEYWORD,
            papers INT, cites INT, recent INT, growth FLOAT, by_year TEXT OPTIONS(indexed = false)
        )".to_string()),
        ("ax_venues", "CREATE TABLE IF NOT EXISTS ax_venues (
            id KEYWORD PRIMARY KEY, papers INT, cites INT, recent INT, by_year TEXT OPTIONS(indexed = false)
        )".to_string()),
        // Autocomplete: phrases of titles, techniques, categories and
        // people, keyed `p:`, `t:`, `c:`, `a:` and their text, completed by
        // a prefix of the key.
        ("ax_complete", "CREATE TABLE IF NOT EXISTS ax_complete (
            id KEYWORD PRIMARY KEY, kind KEYWORD, text KEYWORD, n INT, weight FLOAT, ref KEYWORD
        ) OPTIONS(cluster_by = 'id')".to_string()),
        // Digests: a reader's feeds (what they want, as JSON), the issues
        // made of them, and the summaries written for them (kept, so each
        // is written once).
        ("ax_feeds", "CREATE TABLE IF NOT EXISTS ax_feeds (
            id KEYWORD PRIMARY KEY, owner KEYWORD, name TEXT, config TEXT OPTIONS(indexed = false),
            created TIMESTAMP, updated TIMESTAMP, public BOOL
        )".to_string()),
        ("ax_issues", "CREATE TABLE IF NOT EXISTS ax_issues (
            id KEYWORD PRIMARY KEY, feed KEYWORD, vol INT, since TIMESTAMP, until TIMESTAMP,
            made TIMESTAMP, title TEXT, body TEXT OPTIONS(indexed = false), papers KEYWORD[], ms FLOAT
        )".to_string()),
        ("ax_notes", "CREATE TABLE IF NOT EXISTS ax_notes (
            id KEYWORD PRIMARY KEY, paper KEYWORD, style KEYWORD, text TEXT OPTIONS(indexed = false),
            org KEYWORD, model KEYWORD, at TIMESTAMP, ms FLOAT
        )".to_string()),
        ("ax_answers", "CREATE TABLE IF NOT EXISTS ax_answers (
            id KEYWORD PRIMARY KEY, kind KEYWORD, question TEXT, answer TEXT OPTIONS(indexed = false),
            sources TEXT OPTIONS(indexed = false), model KEYWORD, at TIMESTAMP, ms FLOAT
        )".to_string()),
        // The figures an issue shows: each image copied once from the
        // paper's page on arxiv.org, as base64.
        ("ax_figures", "CREATE TABLE IF NOT EXISTS ax_figures (
            id KEYWORD PRIMARY KEY, paper KEYWORD, fig KEYWORD, k INT, mime KEYWORD,
            data TEXT OPTIONS(indexed = false), bytes INT, w INT, h INT, src KEYWORD, at TIMESTAMP
        )".to_string()),
        ("ax_library", "CREATE TABLE IF NOT EXISTS ax_library (
            id KEYWORD PRIMARY KEY, usr KEYWORD, paper KEYWORD, at TIMESTAMP, note TEXT
        )".to_string()),
        ("ax_state", "CREATE TABLE IF NOT EXISTS ax_state (
            id KEYWORD PRIMARY KEY, val TEXT OPTIONS(indexed = false), n INT, at TIMESTAMP
        )".to_string()),
        ("ax_searches", "CREATE TABLE IF NOT EXISTS ax_searches (
            id KEYWORD PRIMARY KEY, n INT, last TIMESTAMP
        ) OPTIONS(cluster_by = 'id')".to_string()),
    ]
}

/// The words papers write for the same thing, as a synonym list in Solr's
/// format, kept with the papers' table and used by searches with
/// `synonyms => 1`.
pub const SYNONYMS: &str = "llm, large language model
llms, large language models
vlm, vision language model
vlms, vision language models
mllm, multimodal large language model
rag, retrieval augmented generation
rlhf, reinforcement learning from human feedback
dpo, direct preference optimization
grpo, group relative policy optimization
ppo, proximal policy optimization
sft, supervised fine tuning
peft, parameter efficient fine tuning
lora, low rank adaptation
moe, mixture of experts
gnn, graph neural network
gnns, graph neural networks
gcn, graph convolutional network
cnn, convolutional neural network
cnns, convolutional neural networks
rnn, recurrent neural network
lstm, long short term memory
vit, vision transformer
gan, generative adversarial network
gans, generative adversarial networks
vae, variational autoencoder
nerf, neural radiance field
3dgs, gaussian splatting
pinn, physics informed neural network
pinns, physics informed neural networks
ssm, state space model
nlp, natural language processing
nmt, neural machine translation
asr, automatic speech recognition
tts, text to speech
qa, question answering
ir, information retrieval
cv, computer vision
rl, reinforcement learning
marl, multi agent reinforcement learning
icl, in context learning
cot, chain of thought
kg, knowledge graph
kgs, knowledge graphs
ctr, click through rate
recsys, recommender systems
ann, approximate nearest neighbor
mcmc, markov chain monte carlo
gp, gaussian process
gps, gaussian processes
bo, bayesian optimization
sgd, stochastic gradient descent
dft, density functional theory
qcd, quantum chromodynamics
qed, quantum electrodynamics
cmb, cosmic microwave background
gw, gravitational wave
gws, gravitational waves
bh, black hole
bhs, black holes
qec, quantum error correction
vqe, variational quantum eigensolver
qaoa, quantum approximate optimization algorithm
qml, quantum machine learning
mpc, model predictive control
slam, simultaneous localization and mapping
ode, ordinary differential equation
odes, ordinary differential equations
pde, partial differential equation
pdes, partial differential equations
sde, stochastic differential equation
fem, finite element method
mri, magnetic resonance imaging
ehr, electronic health records
xai, explainable artificial intelligence
dp, differential privacy
fl, federated learning
iot, internet of things
llm agents, language agents
genai, generative ai
ai, artificial intelligence
ml, machine learning
dl, deep learning";

/// The tables of the search server; the others are the graph server's
/// (one server may be both).
pub const SEARCH: [&str; 7] = ["ax_authors", "ax_papers", "ax_topics", "ax_concepts", "ax_cats", "ax_venues", "ax_complete"];

pub fn on_search(table: &str) -> bool {
    SEARCH.contains(&table)
}

/// Makes the tables that are not there yet, each on its server, and the
/// synonym list.
pub async fn ensure(db: &Strata, graph: &Strata, model: &str) -> Result<Vec<String>> {
    let have = db.tables().await?;
    let have_g = if graph.url == db.url { have.clone() } else { graph.tables().await? };
    let mut made = Vec::new();
    for (name, sql) in tables(model) {
        let (on, have) = if on_search(name) { (db, &have) } else { (graph, &have_g) };
        if !have.iter().any(|t| t == name) {
            on.sql(&sql).await?;
            made.push(name.to_string());
        }
    }
    let _ = db.sql(&format!("CREATE OR REPLACE SYNONYMS ON ax_papers AS '{}'", SYNONYMS.replace('\'', "''"))).await;
    Ok(made)
}

/// The recommender of what to read next, over the events on the graph
/// server. It has no items' table there (the papers' vectors are on the
/// search server), so the app adds the nearest by meaning itself. Counting
/// six million events takes some minutes, once; then each new event is
/// counted as it is written.
pub async fn ensure_recommender(db: &Strata) -> Result<Option<Value>> {
    let have = db.query("SELECT name FROM _functions WHERE name = :n", serde_json::json!({"n": RECOMMENDER})).await.unwrap_or_default();
    if !have.is_empty() {
        return Ok(None);
    }
    let r = db.sql_long(&format!("CREATE RECOMMENDER {RECOMMENDER} ON ax_events(usr, item, ts) ITEMS ax_nodes OPTIONS (window = 5, history = 50)"), 3600).await?;
    Ok(Some(r))
}

/// How many rows each table holds.
pub async fn counts(db: &Strata, graph: &Strata) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (name, _) in tables(MODEL) {
        let db = if on_search(name) { db } else { graph };
        let n = db.query(&format!("SELECT COUNT(*) AS n FROM {name}"), Value::Null).await.ok().and_then(|r| r.first().and_then(|r| r.get("n").cloned())).unwrap_or(Value::Null);
        out.insert(name.to_string(), n);
    }
    Ok(Value::Object(out))
}
