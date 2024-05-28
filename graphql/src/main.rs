use std::sync::Arc;

use actix_cors::Cors;
use actix_web::{guard, middleware::Logger, web, App, HttpResponse, HttpServer, Result};
use async_graphql::{http::GraphiQLSource, EmptyMutation, EmptySubscription, Schema};
use async_graphql_actix_web::GraphQL;

use trustify_common::{config::Database, db};
use trustify_module_ingestor::graph::Graph;

use crate::schema::query::Query;

mod schema;

struct DB {
    graph: Arc<Graph>,
}

impl DB {
    async fn new(database: &Database) -> anyhow::Result<Self> {
        let db = db::Database::new(&database).await?;
        let graph = Graph::new(db.clone());

        Ok(Self {
            graph: Arc::new(graph.clone()),
        })
    }
}

/// Run the API server
#[derive(clap::Args, Debug)]
pub struct Run {
    #[command(flatten)]
    pub database: Database,

    #[arg(long, env)]
    pub devmode: bool,
}

async fn index_graphiql() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(GraphiQLSource::build().endpoint("/").finish()))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    log::info!("starting HTTP server on port 9090");
    log::info!("GraphiQL playground: http://localhost:8080/graphiql");

    let database = Database {
        username: "postgres".to_string(),
        password: "eggs".to_string(),
        host: "localhost".to_string(),
        port: 5432,
        name: "trustify".to_string(),
    };

    let dbms = DB::new(&database)
        .await
        .expect("error initializing database");

    let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
        .data::<Arc<Graph>>(dbms.graph)
        .finish();

    HttpServer::new(move || {
        let schema = schema.clone();
        App::new()
            .service(
                web::resource("/")
                    .guard(guard::Post())
                    .to(GraphQL::new(schema)),
            )
            .service(web::resource("/").guard(guard::Get()).to(index_graphiql))
            .wrap(Cors::permissive())
            .wrap(Logger::default())
    })
    .workers(2)
    .bind(("127.0.0.1", 9090))?
    .run()
    .await
}
