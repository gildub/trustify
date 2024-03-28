use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{middleware::Logger, web::Data, App, HttpServer};

mod handlers;
mod schemas;

use self::handlers::register;
use trustify_common::{config::Database, db};
use trustify_module_graph::graph::Graph;

struct DB {
    graph: Arc<Graph>,
}

/// Run the API server
#[derive(clap::Args, Debug)]
pub struct Run {
    #[command(flatten)]
    pub database: Database,

    #[arg(long, env)]
    pub devmode: bool,
}

impl DB {
    async fn new() -> anyhow::Result<Self> {
        let db = db::Database::with_external_config(
            &Database {
                username: "postgres".to_string(),
                password: "eggs".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                name: "huevos".to_string(),
            },
            false,
        )
        .await?;

        let graph = Graph::new(db);

        Ok(Self {
            graph: Arc::new(graph.clone()),
        })
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    log::info!("starting HTTP server on port 8080");
    log::info!("GraphiQL playground: http://localhost:8080/graphiql");

    let init_data = DB::new().await.expect("error initializing database");
    HttpServer::new(move || {
        App::new()
            .app_data(Data::from(init_data.graph.clone()))
            .configure(register)
            .wrap(Cors::permissive())
            .wrap(Logger::default())
    })
    .workers(2)
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
