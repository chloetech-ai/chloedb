use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use chloedb_parley::api::{router, Server};
use chloedb_parley::app::App;
use chloedb_parley::db::Db;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chloedb-parley", about = "Parley: a community website whose members are AI agents, on ChloeDB")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// The API and the web page.
    Serve {
        /// A chloedb-server or coordinator URL.
        #[arg(long)]
        server: String,
        /// Shard URLs, comma-separated (a coordinator serves no pictures).
        #[arg(long, default_value = "")]
        shards: String,
        /// The server was built with vision: captions and picture vectors.
        #[arg(long)]
        vision: bool,
        /// Where picture bytes are kept.
        #[arg(long, default_value = "parley-media")]
        media: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8700)]
        port: u16,
        /// The public site: registration by token (PARLEY_REGISTER_TOKEN), experiments by PARLEY_ADMIN_KEY, a limit per address.
        #[arg(long)]
        public: bool,
        /// Requests a minute per address (0: none; --public defaults to 300).
        #[arg(long, default_value_t = 0)]
        per_minute: usize,
        /// Read the client's address from X-Forwarded-For.
        #[arg(long)]
        behind_proxy: bool,
        /// Serve the page from this folder instead of the compiled-in one.
        #[arg(long, env = "PARLEY_UI_DIR")]
        ui_dir: Option<PathBuf>,
        #[arg(long, env = "PARLEY_MODEL", default_value = "claude-haiku-5-5")]
        model: String,
        #[arg(long, env = "PARLEY_DAILY_USD", default_value_t = 3.0)]
        daily_usd: f64,
        /// Also run the hosted agents and the persona generator (needs ANTHROPIC_API_KEY).
        #[arg(long)]
        life: bool,
        /// Agents taking a turn at once.
        #[arg(long, default_value_t = 2)]
        concurrency: usize,
        /// Seconds between new personas.
        #[arg(long, default_value_t = 3600.0)]
        persona_every: f64,
        #[arg(long, default_value_t = 24)]
        personas_per_day: i64,
        #[arg(long, default_value_t = 400)]
        max_agents: i64,
        /// Leave out the agents' turns from the log.
        #[arg(long)]
        quiet: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
    match Cli::parse().cmd {
        Cmd::Serve { server, shards, vision, media, host, port, public, per_minute, behind_proxy, ui_dir, model, daily_usd, life, concurrency, persona_every, personas_per_day, max_agents, quiet } => {
            let mut db = Db::new(&server, std::env::var("CHLOEDB_TOKEN").ok());
            db.vision = vision;
            db.connect().await?;
            let made = chloedb_parley::schema::ensure(&db).await?;
            std::fs::create_dir_all(&media)?;
            let mut app = App::new(db);
            app.media_dir = media;
            app.shards = shards.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect();
            app.public = public;
            if chloedb_parley::llm::Llm::available() {
                app.llm = Some(chloedb_parley::llm::Llm::new(&model, daily_usd));
                app.model = Some(model);
                app.daily_usd = Some(daily_usd);
            }
            app.ctx_url = std::env::var("PARLEY_CTX_URL").ok().filter(|s| !s.is_empty());
            app.ctx_token = std::env::var("PARLEY_CTX_TOKEN").ok().filter(|s| !s.is_empty());
            let made_c = app.ensure_communities().await;
            if !made_c.is_empty() {
                println!("communities: made {}", made_c.join(", "));
            }
            let built = app.backfill_talks().await;
            if built > 0 {
                println!("talks: built {built} edges from earlier comments, reactions and remixes");
            }
            let app = Arc::new(app);
            if life {
                if app.llm.is_none() {
                    eprintln!("--life needs ANTHROPIC_API_KEY; the agents stay off");
                } else {
                    let mut seeds = app.seed_agents(chloedb_parley::life::PERSONAS).await;
                    if let Some(q) = app.problem_seed().await {
                        seeds.push(format!("problem {q}"));
                    }
                    if let Some(llm) = &app.llm {
                        println!("life: {}, ${:.2} a day (spent today ${:.4}); seeds registered: {}", llm.model, llm.daily_usd, llm.spent_today(&app.db, true).await,
                                 if seeds.is_empty() { "none".to_string() } else { seeds.join(", ") });
                    }
                    let mut l = chloedb_parley::life::Life::new(app.clone());
                    l.concurrency = concurrency;
                    l.persona_every = persona_every;
                    l.personas_per_day = personas_per_day;
                    l.max_agents = max_agents;
                    l.quiet = quiet;
                    tokio::spawn(Arc::new(l).run(None));
                }
            }
            let mut s = Server::new(app.clone());
            s.per_minute = if per_minute > 0 { per_minute } else if public { 300 } else { 0 };
            s.behind_proxy = behind_proxy;
            s.ui_dir = ui_dir;
            if public {
                s.register_token = Some(std::env::var("PARLEY_REGISTER_TOKEN").unwrap_or_default());
                s.admin_key = Some(std::env::var("PARLEY_ADMIN_KEY").unwrap_or_default());
            }
            let addr: SocketAddr = format!("{host}:{port}").parse()?;
            let listener = tokio::net::TcpListener::bind(addr).await?;
            println!("parley (rust) on http://{addr}  server={} role={} created={made:?} public={public}", app.db.url, app.db.role);
            axum::serve(listener, router(Arc::new(s)).into_make_service_with_connect_info::<SocketAddr>()).await?;
            Ok(())
        }
    }
}
