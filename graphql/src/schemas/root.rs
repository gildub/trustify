use juniper::{
    graphql_object, graphql_value, EmptySubscription, FieldError, FieldResult, RootNode,
};

use super::advisory::Advisory;
use super::package::Package;
use super::sbom::{Sbom, SbomInput};
use super::vulnerability::Vulnerability;
use trustify_common::db::Transactional;
use trustify_module_graph::graph::Graph;

pub struct Context {
    pub graph: Graph,
}
impl juniper::Context for Context {}

pub struct QueryRoot;

#[graphql_object(Context = Context)]
impl QueryRoot {
    #[graphql(description = "Get single SBOM by location and sha256")]
    async fn sbom(context: &Context, _location: String, _sha256: String) -> FieldResult<Sbom> {
        let loaded_sbom = match context
            .graph
            .get_sbom(_location.as_str(), _sha256.as_str())
            .await
        {
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
        let loaded_sbom = match context.graph.get_sboms(Transactional::None).await {
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

    #[graphql(description = "List of all vulnerabilities")]
    async fn vulnerabilities(context: &Context) -> FieldResult<Vec<Vulnerability>> {
        let loaded_v11y = match context.graph.get_vulnerabilities(Transactional::None).await {
            Ok(loaded_v11y) => loaded_v11y,
            _ => vec![],
        };

        loaded_v11y
            .into_iter()
            .map(|v11y| {
                Ok(Vulnerability {
                    id: v11y.vulnerability.id,
                    identifier: v11y.vulnerability.identifier.to_string(),
                    title: v11y.vulnerability.title.unwrap_or_default(),
                    advisories: vec![],
                })
            })
            .collect()
    }

    #[graphql(description = "List of all advisories")]
    async fn advisories(context: &Context) -> FieldResult<Vec<Advisory>> {
        let loaded_v11y = match context.graph.get_advisories(Transactional::None).await {
            Ok(loaded_v11y) => loaded_v11y,
            _ => vec![],
        };

        loaded_v11y
            .into_iter()
            .map(|advisory| {
                Ok(Advisory {
                    id: advisory.advisory.id,
                    identifier: advisory.advisory.identifier.to_string(),
                    location: advisory.advisory.location.to_string(),
                    sha256: advisory.advisory.sha256.to_string(),
                    title: advisory.advisory.title.unwrap_or_default(),
                })
            })
            .collect()
    }

    #[graphql(description = "List of all packages")]
    async fn packages(context: &Context) -> FieldResult<Vec<Package>> {
        let loaded_package = match context.graph.get_packages(Transactional::None).await {
            Ok(loaded_package) => loaded_package,
            _ => vec![],
        };

        loaded_package
            .into_iter()
            .map(|package| {
                Ok(Package {
                    id: package.package.id,
                    r#type: package.package.r#type.to_string(),
                    namespace: package.package.namespace.unwrap_or("".to_string()),
                    name: package.package.name.to_string(),
                })
            })
            .collect()
    }
}

pub struct MutationRoot;

#[graphql_object(Context = Context)]
impl MutationRoot {
    async fn create_sbom(context: &Context, sbom: SbomInput) -> FieldResult<Sbom> {
        let insert = context
            .graph
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
