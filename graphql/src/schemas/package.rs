use juniper::{graphql_object, GraphQLInputObject};

use crate::schemas::root::Context;

/// Sbom
#[derive(Default, Debug)]
pub struct Package {
    pub id: i32,
    pub r#type: String,
    pub namespace: String,
    pub name: String,
}

#[derive(GraphQLInputObject)]
#[graphql(description = "Package Input")]
pub struct PackageInput {
    pub r#type: String,
    // pub namespace: Option<String>,
    pub namespace: String,
    pub name: String,
}

#[graphql_object(Context = Context)]
impl Package {
    fn id(&self) -> i32 {
        self.id
    }
    fn r#type(&self) -> &str {
        &self.r#type
    }
    fn namespace(&self) -> &str {
        &self.namespace
    }
    fn name(&self) -> &str {
        &self.name
    }
}
