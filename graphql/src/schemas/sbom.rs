use juniper::GraphQLInputObject;

use crate::schemas::root::Context;

/// Sbom
#[derive(Default, Debug)]
pub struct Sbom {
    pub id: i32,
    pub location: String,
    pub sha256: String,
}

#[derive(GraphQLInputObject)]
#[graphql(description = "Sbom Input")]
pub struct SbomInput {
    pub location: String,
    pub sha256: String,
}

#[juniper::graphql_object(Context = Context)]
impl Sbom {
    fn id(&self) -> i32 {
        self.id
    }
    fn location(&self) -> &str {
        &self.location
    }
    fn sha256(&self) -> &str {
        &self.sha256
    }
}
