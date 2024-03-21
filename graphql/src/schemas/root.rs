use juniper::{
    graphql_object, graphql_value, EmptySubscription, FieldError, FieldResult, RootNode,
};
impl juniper::Context for Context {}

use super::sbom::{Sbom, SbomInput};
use trustify_common::db::Transactional;
use trustify_module_graph::graph::Graph;

pub struct Context {
    pub graph: Graph,
}

pub struct QueryRoot;

#[graphql_object(Context = Context)]
impl QueryRoot {
    #[graphql(description = "Get single SBOM by location and sha256")]
    async fn sbom(context: &Context, _location: String, _sha256: String) -> FieldResult<Sbom> {
        let graph = context.graph.clone();

        let loaded_sbom = match graph.get_sbom(_location.as_str(), _sha256.as_str()).await {
            Ok(loaded_sbom) => loaded_sbom,
            _ => None,
        };

        loaded_sbom
            .map(|sbom| {
                Ok(Sbom {
                    id: sbom.sbom.id,
                    location: sbom.sbom.location.to_string(),
                    sha256: sbom.sbom.sha256.to_string(),
                })
            })
            .unwrap_or(Err(FieldError::new(
                "Failed to retrieve sbom",
                graphql_value!(None),
            )))
    }

    #[graphql(description = "List of all sboms")]
    async fn sboms(context: &Context) -> FieldResult<Vec<Sbom>> {
        let graph = context.graph.clone();

        let loaded_sbom = match graph.get_sboms(Transactional::None).await {
            Ok(loaded_sbom) => loaded_sbom,
            _ => vec![],
        };

        loaded_sbom
            .into_iter()
            .map(|sbom| {
                Ok(Sbom {
                    id: sbom.sbom.id,
                    location: sbom.sbom.location.to_string(),
                    sha256: sbom.sbom.sha256.to_string(),
                })
            })
            .collect()
    }
}

pub struct MutationRoot;

#[graphql_object(Context = Context)]
impl MutationRoot {
    async fn create_sbom(context: &Context, sbom: SbomInput) -> FieldResult<Sbom> {
        let conn = context.graph.clone();

        let insert = conn
            .ingest_sbom(&sbom.location, &sbom.sha256, Transactional::None)
            .await;

        match insert {
            Ok(opt_row) => Ok(Sbom {
                id: opt_row.sbom.id,
                location: sbom.location,
                sha256: sbom.sha256,
            }),
            Err(err) => {
                let msg = match err {
                    // TODO - Common dbms errors : Error(err) => err.message,
                    _ => "internal error".to_owned(),
                };
                Err(FieldError::new(
                    "Failed to create new sbom",
                    graphql_value!({ "internal_error": msg }),
                ))
            }
        }
    }
}

pub type Schema = RootNode<'static, QueryRoot, MutationRoot, EmptySubscription<Context>>;

pub fn create_schema() -> Schema {
    Schema::new(QueryRoot, MutationRoot, EmptySubscription::new())
}
