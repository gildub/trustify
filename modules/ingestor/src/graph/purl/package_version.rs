//! Support for *versioned* package.

use crate::graph::{
    error::Error,
    purl::{qualified_package::QualifiedPackageContext, PackageContext},
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use std::fmt::{Debug, Formatter};
use trustify_common::db::Transactional;
use trustify_common::purl::Purl;
use trustify_entity as entity;
use trustify_entity::package_version;
use trustify_entity::qualified_package::Qualifiers;

/// Live context for a package version.
#[derive(Clone)]
pub struct PackageVersionContext<'g> {
    pub package: PackageContext<'g>,
    pub package_version: entity::package_version::Model,
}

impl Debug for PackageVersionContext<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.package_version.fmt(f)
    }
}

impl<'g> PackageVersionContext<'g> {
    pub fn new(package: &PackageContext<'g>, package_version: package_version::Model) -> Self {
        Self {
            package: package.clone(),
            package_version,
        }
    }

    pub async fn ingest_qualified_package<TX: AsRef<Transactional>>(
        &self,
        purl: &Purl,
        tx: TX,
    ) -> Result<QualifiedPackageContext<'g>, Error> {
        if let Some(found) = self.get_qualified_package(purl, &tx).await? {
            return Ok(found);
        }

        // No appropriate qualified package, create one.
        let qualified_package = entity::qualified_package::ActiveModel {
            id: Set(purl.qualifier_uuid()),
            package_version_id: Set(self.package_version.id),
            qualifiers: Set(Qualifiers(purl.qualifiers.clone())),
        };

        let qualified_package = qualified_package
            .insert(&self.package.graph.connection(&tx))
            .await?;

        Ok(QualifiedPackageContext::new(self, qualified_package))
    }

    pub async fn get_qualified_package<TX: AsRef<Transactional>>(
        &self,
        purl: &Purl,
        tx: TX,
    ) -> Result<Option<QualifiedPackageContext<'g>>, Error> {
        let found = entity::qualified_package::Entity::find()
            .filter(entity::qualified_package::Column::PackageVersionId.eq(self.package_version.id))
            .filter(
                entity::qualified_package::Column::Qualifiers
                    .eq(Qualifiers(purl.qualifiers.clone())),
            )
            .one(&self.package.graph.connection(&tx))
            .await?;

        Ok(found.map(|model| QualifiedPackageContext::new(self, model)))
    }

    /// Retrieve known variants of this package version.
    ///
    /// Non-mutating to the fetch.
    pub async fn get_variants<TX: AsRef<Transactional>>(
        &self,
        _pkg: Purl,
        tx: TX,
    ) -> Result<Vec<QualifiedPackageContext>, Error> {
        Ok(entity::qualified_package::Entity::find()
            .filter(entity::qualified_package::Column::PackageVersionId.eq(self.package_version.id))
            .all(&self.package.graph.connection(&tx))
            .await?
            .into_iter()
            .map(|base| QualifiedPackageContext::new(self, base))
            .collect())
    }
}
