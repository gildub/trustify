use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
#[allow(deprecated)]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add generated columns for package_namespace and package_name
        // These columns split the package field to enable indexed lookups:
        // - package_namespace: NULL for packages without '/', otherwise the part before '/'
        // - package_name: the part after '/' if present, otherwise the entire package value
        //
        // Examples:
        //   package = "lodash"           -> namespace=NULL, name="lodash"
        //   package = "npmjs/lodash"     -> namespace="npmjs", name="lodash"
        //   package = "@types/node"      -> namespace="@types", name="node"
        //
        // This maintains compatibility with existing query patterns:
        //   - Match on name only: WHERE package_namespace IS NULL AND package_name = ?
        //   - Match on namespace/name: WHERE package_namespace = ? AND package_name = ?
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE product_status \
                 ADD COLUMN IF NOT EXISTS package_namespace text GENERATED ALWAYS AS (\
                     CASE WHEN package LIKE '%/%' THEN split_part(package, '/', 1) ELSE NULL END\
                 ) STORED",
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE product_status \
                 ADD COLUMN IF NOT EXISTS package_name text GENERATED ALWAYS AS (\
                     CASE WHEN package LIKE '%/%' THEN split_part(package, '/', 2) ELSE package END\
                 ) STORED",
            )
            .await?;

        // Backfill existing rows with UPDATE to trigger recalculation of generated columns
        manager
            .get_connection()
            .execute_unprepared(
                "UPDATE product_status SET package = package WHERE package_namespace IS NULL OR package_name IS NULL",
            )
            .await?;

        // CONCURRENTLY (not supported by SeaORM) to avoid blocking writes
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_product_status_package_lookup \
                 ON product_status (package_namespace, package_name)",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX IF EXISTS idx_product_status_package_lookup")
            .await?;

        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE product_status DROP COLUMN IF EXISTS package_name")
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE product_status DROP COLUMN IF EXISTS package_namespace",
            )
            .await?;

        Ok(())
    }
}
