use juniper::{graphql_object, GraphQLInputObject};

use crate::schemas::root::Context;

/// Advisory
#[derive(Default, Debug)]
pub struct Advisory {
    pub id: i32,
    pub identifier: String,
    pub location: String,
    pub sha256: String,
    pub title: String,
}

#[derive(GraphQLInputObject)]
#[graphql(description = "Advisory Input")]
pub struct AdvisoryInput {
    pub identifier: String,
    pub location: String,
    pub sha256: String,
    pub title: String,
}

#[graphql_object(Context = Context)]
impl Advisory {
    fn id(&self) -> i32 {
        self.id
    }
    fn identifier(&self) -> &str {
        &self.identifier
    }
    fn location(&self) -> &str {
        &self.location
    }
    fn sha256(&self) -> &str {
        &self.sha256
    }
    fn title(&self) -> &str {
        &self.title
    }
}
