use crate::advisory;
use async_graphql::*;
use sea_orm::entity::prelude::*;
// use std::sync::Arc;
// use trustify_common::db;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, SimpleObject)]
// #[graphql(complex)]
#[graphql(concrete(name = "Organization", params()))]
#[sea_orm(table_name = "organization")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub cpe_key: Option<String>,
    pub website: Option<String>,
}

// #[ComplexObject]
// impl Model {
//     async fn advisories(&self, ctx: &Context<'_>) -> Result<Vec<advisory::Model>> {
//         let db: &Arc<db::Database> = ctx.data::<Arc<db::Database>>().unwrap();
//         Ok(self
//             .find_related(advisory::Entity)
//             .all(&db.connection(&db::Transactional::None))
//             .await?)
//     }
// }

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl Related<advisory::Entity> for Entity {
    fn to() -> RelationDef {
        super::advisory::Relation::Organization.def().rev()
    }
}

impl ActiveModelBehavior for ActiveModel {}
