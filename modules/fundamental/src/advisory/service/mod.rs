use sea_orm::{
    ColumnTrait, ColumnTypeTrait, EntityTrait, FromQueryResult, IntoIdentity, QueryFilter,
    QuerySelect, QueryTrait,
};
use sea_query::{ColumnRef, ColumnType, Func, IntoColumnRef, IntoIden, SimpleExpr};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::advisory::model::{AdvisoryDetails, AdvisorySummary};
use crate::Error;
use trustify_common::db::limiter::LimiterAsModelTrait;
use trustify_common::db::query::{Columns, Filtering, Query};
use trustify_common::db::{Database, Transactional};
use trustify_common::id::Id;
use trustify_common::model::{Paginated, PaginatedResults};
use trustify_entity::cvss3::Severity;
use trustify_entity::labels::Labels;
use trustify_entity::{advisory, cvss3};

pub struct AdvisoryService {
    db: Database,
}

impl AdvisoryService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn fetch_advisories<TX: AsRef<Transactional> + Sync + Send>(
        &self,
        search: Query,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<AdvisorySummary>, Error> {
        let connection = self.db.connection(&tx);

        // To be able to ORDER or WHERE using a synthetic column, we must first
        // SELECT col, extra_col FROM (SELECT col, random as extra_col FROM...)
        // which involves mucking about inside the Select<E> to re-target from
        // the original underlying table it expects the entity to live in.
        let inner_query = advisory::Entity::find()
            .left_join(cvss3::Entity)
            .expr_as_(
                SimpleExpr::FunctionCall(Func::avg(SimpleExpr::Column(
                    cvss3::Column::Score.into_column_ref(),
                ))),
                "average_score",
            )
            .expr_as_(
                SimpleExpr::FunctionCall(Func::cust("cvss3_severity".into_identity()).arg(
                    SimpleExpr::FunctionCall(Func::avg(SimpleExpr::Column(
                        cvss3::Column::Score.into_column_ref(),
                    ))),
                )),
                "average_severity",
            )
            .group_by(advisory::Column::Id);

        let mut outer_query = advisory::Entity::find();

        // Alias the inner query as exactly the table the entity is expecting
        // so that column aliases link up correctly.
        QueryTrait::query(&mut outer_query)
            .from_clear()
            .from_subquery(inner_query.into_query(), "advisory".into_identity());

        // And then proceed as usual.
        let limiter = outer_query
            .column_as(
                SimpleExpr::Column(ColumnRef::Column(
                    "average_score".into_identity().into_iden(),
                )),
                "average_score",
            )
            .column_as(
                SimpleExpr::Column(ColumnRef::Column(
                    "average_severity".into_identity().into_iden(),
                ))
                .cast_as("TEXT".into_identity()),
                "average_severity",
            )
            .filtering_with(
                search,
                Columns::from_entity::<advisory::Entity>()
                    .add_column("average_score", ColumnType::Decimal(None).def())
                    .add_column(
                        "average_severity",
                        ColumnType::Enum {
                            name: "cvss3_severity".into_identity().into_iden(),
                            variants: vec![
                                "none".into_identity().into_iden(),
                                "low".into_identity().into_iden(),
                                "medium".into_identity().into_iden(),
                                "high".into_identity().into_iden(),
                                "critical".into_identity().into_iden(),
                            ],
                        }
                        .def(),
                    )
                    .translator(|f, op, v| match (f, v) {
                        // v = "" for all sort fields
                        ("average_severity", "") => Some(format!("average_score:{op}")),
                        _ => None,
                    }),
            )?
            .limiting_as::<AdvisoryCatcher>(&connection, paginated.offset, paginated.limit);

        let total = limiter.total().await?;

        let items = limiter.fetch().await?;

        let averages: Vec<_> = items
            .iter()
            .map(|e| (e.average_score, e.average_severity.clone()))
            .collect();

        let entities: Vec<_> = items
            .into_iter()
            .map(|e| advisory::Model {
                id: e.id,
                identifier: e.identifier,
                issuer_id: e.issuer_id,
                labels: e.labels,
                sha256: e.sha256,
                published: e.published,
                modified: e.modified,
                withdrawn: e.withdrawn,
                title: e.title,
            })
            .collect();

        Ok(PaginatedResults {
            total,
            items: AdvisorySummary::from_entities(&entities, &averages, &connection).await?,
        })
    }

    pub async fn fetch_advisory<TX: AsRef<Transactional> + Sync + Send>(
        &self,
        hash_key: Id,
        tx: TX,
    ) -> Result<Option<AdvisoryDetails>, Error> {
        let connection = self.db.connection(&tx);

        // To be able to ORDER or WHERE using a synthetic column, we must first
        // SELECT col, extra_col FROM (SELECT col, random as extra_col FROM...)
        // which involves mucking about inside the Select<E> to re-target from
        // the original underlying table it expects the entity to live in.
        let inner_query = advisory::Entity::find()
            .left_join(cvss3::Entity)
            .expr_as_(
                SimpleExpr::FunctionCall(Func::avg(SimpleExpr::Column(
                    cvss3::Column::Score.into_column_ref(),
                ))),
                "average_score",
            )
            .expr_as_(
                SimpleExpr::FunctionCall(Func::cust("cvss3_severity".into_identity()).arg(
                    SimpleExpr::FunctionCall(Func::avg(SimpleExpr::Column(
                        cvss3::Column::Score.into_column_ref(),
                    ))),
                )),
                "average_severity",
            )
            .group_by(advisory::Column::Id);

        let mut outer_query = advisory::Entity::find();

        // Alias the inner query as exactly the table the entity is expecting
        // so that column aliases link up correctly.
        QueryTrait::query(&mut outer_query)
            .from_clear()
            .from_subquery(inner_query.into_query(), "advisory".into_identity());

        let results = outer_query
            .column_as(
                SimpleExpr::Column(ColumnRef::Column(
                    "average_score".into_identity().into_iden(),
                )),
                "average_score",
            )
            .column_as(
                SimpleExpr::Column(ColumnRef::Column(
                    "average_severity".into_identity().into_iden(),
                ))
                .cast_as("TEXT".into_identity()),
                "average_severity",
            )
            .filter(match hash_key {
                Id::Uuid(uuid) => advisory::Column::Id.eq(uuid),
                Id::Sha256(hash) => advisory::Column::Sha256.eq(hash),
                _ => return Err(Error::UnsupportedHashAlgorithm),
            })
            .into_model::<AdvisoryCatcher>()
            .one(&connection)
            .await?;

        if let Some(advisory) = results {
            let entity = advisory::Model {
                id: advisory.id,
                identifier: advisory.identifier,
                issuer_id: advisory.issuer_id,
                labels: advisory.labels,
                sha256: advisory.sha256,
                published: advisory.published,
                modified: advisory.modified,
                withdrawn: advisory.withdrawn,
                title: advisory.title,
            };

            let average_score = advisory.average_score;
            let average_severity = advisory.average_severity;

            Ok(Some(
                AdvisoryDetails::from_entity(&entity, average_score, average_severity, &connection)
                    .await?,
            ))
        } else {
            Ok(None)
        }
    }
}

#[derive(FromQueryResult, Debug)]
struct AdvisoryCatcher {
    pub id: Uuid,
    pub identifier: String,
    pub issuer_id: Option<i32>,
    pub labels: Labels,
    pub sha256: String,
    pub published: Option<OffsetDateTime>,
    pub modified: Option<OffsetDateTime>,
    pub withdrawn: Option<OffsetDateTime>,
    pub title: Option<String>,
    // all of advisory, plus some.
    pub average_score: Option<f64>,
    pub average_severity: Option<Severity>,
}

#[cfg(test)]
mod test;
