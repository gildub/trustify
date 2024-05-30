use std::sync::Arc;

use async_graphql::{Context, FieldError, FieldResult, Object};
use uuid::Uuid;

use trustify_common::db::Transactional;
use trustify_entity::{
    advisory::Model as Advisory, organization::Model as Organization, sbom::Model as Sbom,
};
use trustify_module_ingestor::graph::Graph;

pub struct Query;

#[Object]
impl Query {
    async fn get_advisory_by_id<'a>(&self, ctx: &Context<'a>, id: i32) -> FieldResult<Advisory> {
        let graph = ctx.data::<Arc<Graph>>().unwrap();
        let advisory = graph.get_advisory_by_id(id, Transactional::None).await;

        match advisory {
            Ok(Some(advisory)) => Ok(Advisory {
                id: advisory.advisory.id,
                identifier: advisory.advisory.identifier,
                organization_id: advisory.advisory.organization_id,
                location: advisory.advisory.location,
                sha256: advisory.advisory.sha256,
                published: advisory.advisory.published,
                modified: advisory.advisory.modified,
                withdrawn: advisory.advisory.withdrawn,
                title: advisory.advisory.title,
            }),
            Ok(None) => Err(FieldError::new("Advisory not found")),
            Err(err) => Err(FieldError::from(err)),
        }
    }

    async fn get_advisories<'a>(&self, ctx: &Context<'a>) -> FieldResult<Vec<Advisory>> {
        let graph = ctx.data::<Arc<Graph>>().unwrap();
        let advisories = match graph.get_advisories(Transactional::None).await {
            Ok(sbom) => sbom,
            _ => vec![],
        };

        advisories
            .into_iter()
            .map(|advisory| {
                Ok(Advisory {
                    id: advisory.advisory.id,
                    identifier: advisory.advisory.identifier,
                    organization_id: advisory.advisory.organization_id,
                    location: advisory.advisory.location,
                    sha256: advisory.advisory.sha256,
                    published: advisory.advisory.published,
                    modified: advisory.advisory.modified,
                    withdrawn: advisory.advisory.withdrawn,
                    title: advisory.advisory.title,
                })
            })
            .collect()
    }

    async fn get_organization_by_name<'a>(
        &self,
        ctx: &Context<'a>,
        name: String,
    ) -> FieldResult<Organization> {
        let graph = ctx.data::<Arc<Graph>>().unwrap();
        let organization = graph
            .get_organization_by_name(name, Transactional::None)
            .await;

        match organization {
            Ok(Some(organization)) => Ok(Organization {
                id: organization.organization.id,
                name: organization.organization.name,
                cpe_key: organization.organization.cpe_key,
                website: organization.organization.website,
            }),
            Ok(None) => Err(FieldError::new("Organization not found")),
            Err(err) => Err(FieldError::from(err)),
        }
    }
    async fn get_sbom_by_id<'a>(&self, ctx: &Context<'a>, id: Uuid) -> FieldResult<Sbom> {
        let graph = ctx.data::<Arc<Graph>>().unwrap();
        let sbom = graph.locate_sbom_by_id(id, Transactional::None).await;

        match sbom {
            Ok(Some(sbom_context)) => Ok(Sbom {
                sbom_id: sbom_context.sbom.sbom_id,
                node_id: sbom_context.sbom.node_id,
                location: sbom_context.sbom.location,
                sha256: sbom_context.sbom.sha256,
                document_id: sbom_context.sbom.document_id,
                published: sbom_context.sbom.published,
                authors: sbom_context.sbom.authors,
            }),
            Ok(None) => Err(FieldError::new("SBOM not found")),
            Err(err) => Err(FieldError::from(err)),
        }
    }

    async fn sboms_by_location<'a>(
        &self,
        ctx: &Context<'a>,
        location: String,
    ) -> FieldResult<Vec<Sbom>> {
        let graph = ctx.data::<Arc<Graph>>().unwrap();
        let sboms = match graph
            .locate_sboms_by_location(&location, Transactional::None)
            .await
        {
            Ok(sbom) => sbom,
            _ => vec![],
        };

        sboms
            .into_iter()
            .map(|sbom| {
                Ok(Sbom {
                    sbom_id: sbom.sbom.sbom_id,
                    node_id: sbom.sbom.node_id,
                    location: sbom.sbom.location,
                    sha256: sbom.sbom.sha256,
                    document_id: sbom.sbom.document_id,
                    published: sbom.sbom.published,
                    authors: sbom.sbom.authors,
                })
            })
            .collect()
    }
}
