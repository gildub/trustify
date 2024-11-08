// use crate::Error;
use sea_orm::{
    prelude::ConnectionTrait, ColumnTrait, DbErr, EntityOrSelect, EntityTrait, QueryFilter,
    QueryOrder, QueryResult, QuerySelect, QueryTrait, Statement,
};
use std::collections::HashMap;
use tracing::instrument;
use trustify_common::{
    db::{
        query::{Query, Value},
        Database, Transactional,
    },
    model::{Paginated, PaginatedResults},
};

use crate::model::{
    AnalysisStatus, AncNode, AncestorSummary, DepNode, DepSummary, GraphMap, PackageNode,
};
use crate::Error;
use petgraph::algo::is_cyclic_directed;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::{NodeIndexable, VisitMap, Visitable};
use petgraph::Direction;
use sea_query::Order;
use std::str::FromStr;
use trustify_common::db::query::Filtering;
use trustify_common::db::ConnectionOrTransaction;
use trustify_common::purl::Purl;
use trustify_entity::relationship::Relationship;
use trustify_entity::{sbom, sbom_node};

pub struct AnalysisService {
    db: Database,
}

pub fn dep_nodes(
    graph: &petgraph::Graph<PackageNode, Relationship, petgraph::Directed>,
    node: NodeIndex,
) -> Vec<DepNode> {
    let mut depnodes = Vec::new();
    fn dfs(
        graph: &petgraph::Graph<PackageNode, Relationship, petgraph::Directed>,
        node: NodeIndex,
        depnodes: &mut Vec<DepNode>,
    ) {
        for neighbor in graph.neighbors_directed(node, Direction::Incoming) {
            if let Some(dep_packagenode) = graph.node_weight(neighbor).cloned() {
                let dep_node = DepNode {
                    sbom_id: dep_packagenode.sbom_id,
                    node_id: dep_packagenode.node_id,
                    purl: dep_packagenode.purl.to_string(),
                    name: dep_packagenode.name.to_string(),
                    deps: dep_nodes(graph, neighbor),
                };
                depnodes.push(dep_node);
            } else {
                log::warn!(
                    "Processing descendants node weight for neighbor {:?} not found",
                    neighbor
                );
            }
        }
    }
    dfs(graph, node, &mut depnodes);
    depnodes
}

pub fn ancestor_nodes(
    graph: &petgraph::Graph<PackageNode, Relationship, petgraph::Directed>,
    node: NodeIndex,
) -> Vec<AncNode> {
    let mut discovered = graph.visit_map();
    let mut ancestor_nodes = Vec::new();
    let mut stack = Vec::new();

    stack.push(graph.from_index(node.index()));

    while let Some(node) = stack.pop() {
        if discovered.visit(node) {
            for succ in graph.neighbors_directed(node, Direction::Outgoing) {
                if !discovered.is_visited(&succ) {
                    if let Some(anc_packagenode) = graph.node_weight(succ).cloned() {
                        let anc_node = AncNode {
                            sbom_id: anc_packagenode.sbom_id,
                            node_id: anc_packagenode.node_id,
                            purl: anc_packagenode.purl,
                            name: anc_packagenode.name,
                        };
                        ancestor_nodes.push(anc_node);
                        stack.push(succ);
                    } else {
                        log::warn!("Processing ancestors, node value for {:?} not found", succ);
                    }
                }
            }
            if graph.neighbors_directed(node, Direction::Outgoing).count() == 0 {
                continue; // we are at the root
            }
        }
    }
    ancestor_nodes
}

pub async fn get_relationships(
    connection: &ConnectionOrTransaction<'_>,
    distinct_sbom_id: &str,
) -> Result<Vec<QueryResult>, DbErr> {
    // TODO: We may convert this to sea_orm at some point though keeping this in its current form, to highlight
    //       there is no rewriting of the query (in sea_orm or sql) that will significantly speed this up eg.
    //       most of the time is spent resolving pURL using get_purl() which performs a sql join. Optimising
    //       that or consolidating/materialising those tables might be a better approach.
    let sql = format!(
        r#"
        SELECT
            sbom.document_id,
            sbom.sbom_id,
            sbom.published::text,
            t1.node_id AS left_node_id,
            get_purl(t1.qualified_purl_id) AS left_qualified_purl,
            package_relates_to_package.relationship,
            t2.node_id AS right_node_id,
            get_purl(t2.qualified_purl_id) AS right_qualified_purl,
            product.name AS product_name,
            product_version.version AS product_version
        FROM
            sbom
        LEFT JOIN
            product_version ON sbom.sbom_id = product_version.sbom_id
        LEFT JOIN
            product ON product_version.product_id = product.id
        LEFT JOIN
            package_relates_to_package ON sbom.sbom_id = package_relates_to_package.sbom_id
        LEFT JOIN
            sbom_package_purl_ref t1 ON sbom.sbom_id = t1.sbom_id AND t1.node_id = package_relates_to_package.left_node_id
        LEFT JOIN
            sbom_package_purl_ref t2 ON sbom.sbom_id = t2.sbom_id AND t2.node_id = package_relates_to_package.right_node_id
        WHERE
            package_relates_to_package.relationship IN (0, 1, 8, 14)
            AND sbom.sbom_id = '{}';
        "#,
        distinct_sbom_id
    );

    // TODO: there might be better ways to stream this result
    let results: Vec<QueryResult> = connection
        .query_all(Statement::from_string(
            connection.get_database_backend(),
            sql,
        ))
        .await?
        .into_iter()
        .collect();

    Ok(results)
}

pub async fn load_graphs(
    connection: &ConnectionOrTransaction<'_>,
    distinct_sbom_ids: &Vec<String>,
) {
    let graph_map = GraphMap::get_instance();
    {
        for distinct_sbom_id in distinct_sbom_ids {
            if !graph_map.read().contains_key(distinct_sbom_id) {
                // lazy load graphs
                let mut g: Graph<PackageNode, Relationship, petgraph::Directed> = Graph::new();

                match get_relationships(connection, &distinct_sbom_id.to_string()).await {
                    Ok(results) => {
                        let mut nodes = HashMap::new();

                        for row in results {
                            let (
                                sbom_published,
                                document_id,
                                product_name,
                                product_version,
                                left_node_id,
                                left_purl_string,
                                right_node_id,
                                right_purl_string,
                                relationship,
                            ) = {
                                let default_value = "NOVALUE".to_string(); // TODO: this eventually will have different defaults.
                                (
                                    row.try_get("", "published")
                                        .unwrap_or_else(|_| default_value.clone()),
                                    row.try_get("", "document_id")
                                        .unwrap_or_else(|_| default_value.clone()),
                                    row.try_get("", "product_name")
                                        .unwrap_or_else(|_| default_value.clone()),
                                    row.try_get("", "product_version")
                                        .unwrap_or_else(|_| default_value.clone()),
                                    row.try_get("", "left_node_id")
                                        .unwrap_or(default_value.clone()),
                                    row.try_get("", "left_qualified_purl")
                                        .unwrap_or(default_value.clone()),
                                    row.try_get("", "right_node_id")
                                        .unwrap_or(default_value.clone()),
                                    row.try_get("", "right_qualified_purl")
                                        .unwrap_or(default_value),
                                    row.try_get("", "relationship")
                                        .unwrap_or(Relationship::ContainedBy),
                                )
                            };

                            if left_purl_string == "NOVALUE" {
                                log::debug!("sbom:{} has packages with no pURL.", document_id);
                                continue;
                            }

                            let p1 = match nodes.get(&left_purl_string) {
                                Some(&node_index) => node_index, // already exists
                                None => {
                                    let left_purl = match Purl::from_str(&left_purl_string) {
                                        Ok(purl) => purl,
                                        Err(e) => {
                                            log::warn!("Error parsing Purl: {}", e);
                                            return;
                                        }
                                    };
                                    let new_node = PackageNode {
                                        sbom_id: distinct_sbom_id.clone(),
                                        node_id: left_node_id.clone(),
                                        purl: left_purl_string.clone(),
                                        name: left_purl.name.clone(),
                                        published: sbom_published.clone(),
                                        document_id: document_id.clone(),
                                        product_name: product_name.clone(),
                                        product_version: product_version.clone(),
                                    };
                                    let i = g.add_node(new_node);
                                    nodes.insert(left_purl_string.clone(), i);
                                    i
                                }
                            };

                            if right_purl_string == "NOVALUE" {
                                log::debug!("sbom:{} has packages with no pURL.", document_id);
                                continue;
                            }
                            let p2 = match nodes.get(&right_purl_string) {
                                Some(&node_index) => node_index, // already exists
                                None => {
                                    let right_purl = match Purl::from_str(&right_purl_string) {
                                        Ok(purl) => purl,
                                        Err(e) => {
                                            log::warn!("Error parsing Purl: {}", e);
                                            return;
                                        }
                                    };
                                    let new_node = PackageNode {
                                        sbom_id: distinct_sbom_id.clone(),
                                        node_id: right_node_id.clone(),
                                        purl: right_purl_string.clone(),
                                        name: right_purl.name.clone(),
                                        published: sbom_published.clone(),
                                        document_id: document_id.clone(),
                                        product_name: product_name.clone(),
                                        product_version: product_version.clone(),
                                    };
                                    let i = g.add_node(new_node);
                                    nodes.insert(right_purl_string.clone(), i);
                                    i
                                }
                            };

                            g.add_edge(p1, p2, relationship);
                        }
                        graph_map.write().insert(distinct_sbom_id.to_string(), g);
                    }
                    Err(err) => {
                        log::error!("Error fetching graph relationships: {}", err);
                    }
                }
            }
        }
    }
}

impl AnalysisService {
    pub fn new(db: Database) -> Self {
        GraphMap::get_instance();
        Self { db }
    }

    pub async fn load_graphs<TX: AsRef<Transactional>>(
        &self,
        distinct_sbom_ids: Vec<String>,
        tx: TX,
    ) -> Result<(), Error> {
        let connection = self.db.connection(&tx);
        load_graphs(&connection, &distinct_sbom_ids).await;

        Ok(())
    }
    pub async fn load_all_graphs<TX: AsRef<Transactional>>(&self, tx: TX) -> Result<(), Error> {
        let connection = self.db.connection(&tx);
        // retrieve all sboms in trustify
        let distinct_sbom_ids = sbom::Entity::find()
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        Ok(())
    }

    pub async fn clear_all_graphs<TX: AsRef<Transactional>>(&self, _tx: TX) -> Result<(), Error> {
        let graph_manager = GraphMap::get_instance();
        let mut manager = graph_manager.write();
        manager.clear();
        Ok(())
    }

    pub async fn status<TX: AsRef<Transactional>>(&self, tx: TX) -> Result<AnalysisStatus, Error> {
        let connection = self.db.connection(&tx);
        let distinct_sbom_ids = sbom::Entity::find()
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?;

        let graph_manager = GraphMap::get_instance();
        let manager = graph_manager.read();
        Ok(AnalysisStatus {
            sbom_count: distinct_sbom_ids.len() as i32,
            graph_count: manager.len() as i32,
        })
    }

    #[instrument(skip(self, tx), err)]
    pub async fn retrieve_root_components<TX: AsRef<Transactional>>(
        &self,
        query: Query,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<AncestorSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_name_subquery = sbom_node::Entity::find()
            .filtering(query.clone())?
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                graph.node_weight(i).is_some_and(|node| {
                                    query.apply(&HashMap::from([
                                        ("sbom_id", Value::String(&node.sbom_id)),
                                        ("node_id", Value::String(&node.node_id)),
                                        ("name", Value::String(&node.name)),
                                    ]))
                                })
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(AncestorSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        ancestors: ancestor_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        Ok(paginated.paginate_array(&components))
    }

    #[instrument(skip(self, tx), err)]
    pub async fn retrieve_root_components_by_name<TX: AsRef<Transactional>>(
        &self,
        component_name: String,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<AncestorSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_exact_name_subquery = sbom_node::Entity::find()
            .filter(sbom_node::Column::Name.eq(component_name.as_str()))
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_exact_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                if let Some(node) = graph.node_weight(i) {
                                    node.name.eq(&component_name.to_string()) // Use eq for exact match
                                } else {
                                    false // Return false if the node does not exist
                                }
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(AncestorSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        ancestors: ancestor_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        Ok(paginated.paginate_array(&components))
    }

    #[instrument(skip(self, tx), err)]
    pub async fn retrieve_root_components_by_purl<TX: AsRef<Transactional>>(
        &self,
        component_purl: Purl,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<AncestorSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_exact_name_subquery = sbom_node::Entity::find()
            .filter(sbom_node::Column::Name.eq(component_purl.name.as_str()))
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_exact_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                if let Some(node) = graph.node_weight(i) {
                                    match Purl::from_str(&node.purl).map_err(Error::Purl) {
                                        Ok(purl) => purl == component_purl,
                                        Err(err) => {
                                            log::warn!(
                                                "Error retrieving purl from analysis graph {}",
                                                err
                                            );
                                            false
                                        }
                                    }
                                } else {
                                    false // Return false if the node does not exist
                                }
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(AncestorSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        ancestors: ancestor_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        // TODO: limiting on Vec
        let total = components.len() as u64;
        Ok(PaginatedResults {
            items: components,
            total,
        })
    }

    #[instrument(skip(self, tx), err)]
    pub async fn retrieve_deps<TX: AsRef<Transactional>>(
        &self,
        query: Query,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<DepSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_name_subquery = sbom_node::Entity::find()
            .filtering(query.clone())?
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                graph.node_weight(i).is_some_and(|node| {
                                    query.apply(&HashMap::from([
                                        ("sbom_id", Value::String(&node.sbom_id)),
                                        ("node_id", Value::String(&node.node_id)),
                                        ("name", Value::String(&node.name)),
                                    ]))
                                })
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(DepSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        deps: dep_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        Ok(paginated.paginate_array(&components))
    }

    pub async fn retrieve_deps_by_name<TX: AsRef<Transactional>>(
        &self,
        component_name: String,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<DepSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_exact_name_subquery = sbom_node::Entity::find()
            .filter(sbom_node::Column::Name.eq(component_name.as_str()))
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_exact_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                if let Some(node) = graph.node_weight(i) {
                                    node.name.eq(&component_name.to_string())
                                // Use eq for exact match
                                } else {
                                    false // Return false if the node does not exist
                                }
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(DepSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        deps: dep_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        Ok(paginated.paginate_array(&components))
    }

    pub async fn retrieve_deps_by_purl<TX: AsRef<Transactional>>(
        &self,
        component_purl: Purl,
        paginated: Paginated,
        tx: TX,
    ) -> Result<PaginatedResults<DepSummary>, Error> {
        let connection = self.db.connection(&tx);

        let search_sbom_node_exact_name_subquery = sbom_node::Entity::find()
            .filter(sbom_node::Column::Name.eq(component_purl.name.as_str()))
            .select_only()
            .column(sbom_node::Column::SbomId)
            .distinct()
            .into_query();
        let distinct_sbom_ids: Vec<String> = sbom::Entity::find()
            .filter(sbom::Column::SbomId.in_subquery(search_sbom_node_exact_name_subquery))
            .select()
            .order_by(sbom::Column::DocumentId, Order::Asc)
            .order_by(sbom::Column::Published, Order::Desc)
            .all(&connection)
            .await?
            .into_iter()
            .map(|record| record.sbom_id.to_string()) // Assuming sbom_id is of type String
            .collect();

        load_graphs(&connection, &distinct_sbom_ids).await;

        let mut components = Vec::new();
        let graph_manager = GraphMap::get_instance();
        {
            // RwLock for reading hashmap<graph>
            let graph_read_guard = graph_manager.read();
            for distinct_sbom_id in &distinct_sbom_ids {
                if let Some(graph) = graph_read_guard.get(distinct_sbom_id.to_string().as_str()) {
                    if !is_cyclic_directed(graph) {
                        // Iterate over matching node indices and process them directly
                        graph
                            .node_indices()
                            .filter(|&i| {
                                if let Some(node) = graph.node_weight(i) {
                                    match Purl::from_str(&node.purl).map_err(Error::Purl) {
                                        Ok(purl) => purl == component_purl,
                                        Err(err) => {
                                            log::warn!(
                                                "Error retrieving purl from analysis graph {}",
                                                err
                                            );
                                            false
                                        }
                                    }
                                } else {
                                    false // Return false if the node does not exist
                                }
                            })
                            .for_each(|node_index| {
                                if let Some(find_match_package_node) = graph.node_weight(node_index)
                                {
                                    log::debug!("matched!");
                                    components.push(DepSummary {
                                        sbom_id: find_match_package_node.sbom_id.to_string(),
                                        node_id: find_match_package_node.node_id.to_string(),
                                        purl: find_match_package_node.purl.to_string(),
                                        name: find_match_package_node.name.to_string(),
                                        published: find_match_package_node.published.to_string(),
                                        document_id: find_match_package_node
                                            .document_id
                                            .to_string(),
                                        product_name: find_match_package_node
                                            .product_name
                                            .to_string(),
                                        product_version: find_match_package_node
                                            .product_version
                                            .to_string(),
                                        deps: dep_nodes(graph, node_index),
                                    });
                                }
                            });
                    } else {
                        log::warn!(
                            "analysis graph of sbom {} has circular references!",
                            distinct_sbom_id
                        );
                    }
                }
            }
        }

        Ok(paginated.paginate_array(&components))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use test_context::test_context;
    use test_log::test;
    use trustify_common::model::Paginated;
    use trustify_test_context::TrustifyContext;

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_analysis_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json", "spdx/simple.json"])
            .await?; //double ingestion intended

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_root_components(Query::q("DD"), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .purl,
            "pkg://rpm/redhat/AA@0.0.0?arch=src".to_string()
        );
        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .node_id,
            "SPDXRef-AA".to_string()
        );
        assert_eq!(analysis_graph.total, 1);

        let analysis_graph = service
            .retrieve_root_components(Query::q("EE"), Paginated::default(), ())
            .await
            .unwrap();
        Ok(assert_eq!(analysis_graph.total, 0)) //TODO: it maybe implied that a node with no relationship is a root ?
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_analysis_cyclonedx_service(
        ctx: &TrustifyContext,
    ) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["cyclonedx/simple.json", "cyclonedx/simple.json"])
            .await?; //double ingestion intended

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_root_components(Query::q("DD"), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .purl,
            "pkg://rpm/redhat/AA@0.0.0?arch=src".to_string()
        );
        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .node_id,
            "AA".to_string()
        );
        assert_eq!(analysis_graph.total, 1);

        let analysis_graph = service
            .retrieve_root_components(Query::q("EE"), Paginated::default(), ())
            .await
            .unwrap();
        Ok(assert_eq!(analysis_graph.total, 0)) //TODO: it maybe implied that a node with no relationship is a root ?
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_by_name_analysis_service(
        ctx: &TrustifyContext,
    ) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_root_components_by_name("B".to_string(), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .purl,
            "pkg://rpm/redhat/A@0.0.0?arch=src".to_string()
        );
        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .node_id,
            "SPDXRef-A".to_string()
        );
        Ok(assert_eq!(analysis_graph.total, 1))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_by_purl_analysis_service(
        ctx: &TrustifyContext,
    ) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let component_purl: Purl =
            Purl::from_str("pkg://rpm/redhat/B@0.0.0").map_err(Error::Purl)?;

        let analysis_graph = service
            .retrieve_root_components_by_purl(component_purl, Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .purl,
            "pkg://rpm/redhat/A@0.0.0?arch=src".to_string()
        );
        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .node_id,
            "SPDXRef-A".to_string()
        );
        Ok(assert_eq!(analysis_graph.total, 1))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_quarkus_analysis_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents([
            "spdx/quarkus-bom-3.2.11.Final-redhat-00001.json",
            "spdx/quarkus-bom-3.2.12.Final-redhat-00002.json",
        ])
        .await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_root_components(Query::q("spymemcached"), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(analysis_graph.items.last().unwrap().ancestors.last().unwrap().purl,
                   "pkg://maven/com.redhat.quarkus.platform/quarkus-bom@3.2.12.Final-redhat-00002?type=pom&repository_url=https://maven.repository.redhat.com/ga/".to_string()
        );
        assert_eq!(
            analysis_graph
                .items
                .last()
                .unwrap()
                .ancestors
                .last()
                .unwrap()
                .node_id,
            "SPDXRef-e24fec28-1001-499c-827f-2e2e5f2671b5".to_string()
        );

        Ok(assert_eq!(analysis_graph.total, 2))
    }

    // TODO: this test passes when run individually.
    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    #[ignore]
    async fn test_status_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());
        let _load_all_graphs = service.load_all_graphs(()).await;
        let analysis_status = service.status(()).await.unwrap();

        assert_eq!(analysis_status.sbom_count, 1);
        assert_eq!(analysis_status.graph_count, 1);

        let _clear_all_graphs = service.clear_all_graphs(()).await;

        ctx.ingest_documents([
            "spdx/quarkus-bom-3.2.11.Final-redhat-00001.json",
            "spdx/quarkus-bom-3.2.12.Final-redhat-00002.json",
        ])
        .await?;

        let analysis_status = service.status(()).await.unwrap();

        assert_eq!(analysis_status.sbom_count, 3);
        assert_eq!(analysis_status.graph_count, 0);

        Ok(())
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_deps_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps(Query::q("AA"), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(analysis_graph.total, 1);

        let analysis_graph = service
            .retrieve_root_components(Query::q("EE"), Paginated::default(), ())
            .await
            .unwrap();
        Ok(assert_eq!(analysis_graph.total, 0)) //TODO: should this not match with no root_components ?
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_deps_cyclonedx_service(
        ctx: &TrustifyContext,
    ) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["cyclonedx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps(Query::q("AA"), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(analysis_graph.total, 1);

        let analysis_graph = service
            .retrieve_root_components(Query::q("EE"), Paginated::default(), ())
            .await
            .unwrap();
        Ok(assert_eq!(analysis_graph.total, 0)) //TODO: should this not match with no root_components ?
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_by_name_deps_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps_by_name("A".to_string(), Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph.items[0].purl,
            "pkg://rpm/redhat/A@0.0.0?arch=src".to_string()
        );
        Ok(assert_eq!(analysis_graph.total, 1))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_simple_by_purl_deps_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/simple.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let component_purl: Purl =
            Purl::from_str("pkg://rpm/redhat/AA@0.0.0?arch=src").map_err(Error::Purl)?;

        let analysis_graph = service
            .retrieve_deps_by_purl(component_purl, Paginated::default(), ())
            .await
            .unwrap();

        assert_eq!(
            analysis_graph.items[0].purl,
            "pkg://rpm/redhat/AA@0.0.0?arch=src".to_string()
        );

        Ok(assert_eq!(analysis_graph.total, 1))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_quarkus_deps_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents([
            "spdx/quarkus-bom-3.2.11.Final-redhat-00001.json",
            "spdx/quarkus-bom-3.2.12.Final-redhat-00002.json",
        ])
        .await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps(Query::q("spymemcached"), Paginated::default(), ())
            .await
            .unwrap();

        Ok(assert_eq!(analysis_graph.total, 2))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_circular_deps_cyclonedx_service(
        ctx: &TrustifyContext,
    ) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["cyclonedx/cyclonedx-circular.json"])
            .await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps_by_name("junit-bom".to_string(), Paginated::default(), ())
            .await
            .unwrap();

        Ok(assert_eq!(analysis_graph.total, 0))
    }

    #[test_context(TrustifyContext)]
    #[test(tokio::test)]
    async fn test_circular_deps_spdx_service(ctx: &TrustifyContext) -> Result<(), anyhow::Error> {
        ctx.ingest_documents(["spdx/loop.json"]).await?;

        let service = AnalysisService::new(ctx.db.clone());

        let analysis_graph = service
            .retrieve_deps_by_name("A".to_string(), Paginated::default(), ())
            .await
            .unwrap();

        Ok(assert_eq!(analysis_graph.total, 0))
    }
}
