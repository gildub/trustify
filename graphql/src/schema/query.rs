use crate::schema::*;
use async_graphql::MergedObject;

#[derive(MergedObject, Default)]
pub struct Query(
    advisory::AdvisoryQuery,
    organization::OrganizationQuery,
    sbom::SbomQuery,
);
