// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use hawdb::{
    Database, PreparedReadLimits, ProjectionFeedLimits, ProjectionPublicationError,
    ProjectionReadScopes, Value,
};
use std::num::NonZeroUsize;
fn limits() -> PreparedReadLimits {
    let n = |x| NonZeroUsize::new(x).unwrap();
    PreparedReadLimits {
        dependency_entries: n(32),
        dependency_bytes: n(8192),
        statement_rows: n(8),
        statement_payload_bytes: n(8192),
    }
}
fn budget() -> ProjectionFeedLimits {
    ProjectionFeedLimits {
        work: NonZeroUsize::new(128).unwrap(),
        items: NonZeroUsize::new(16).unwrap(),
        bytes: NonZeroUsize::new(8192).unwrap(),
    }
}

#[test]
fn relationship_scope_detects_unselected_owner_edges_and_allows_other_types() {
    for (related, effect) in [false, true]
        .into_iter()
        .flat_map(|related| ["create", "property", "delete"].map(|effect| (related, effect)))
    {
        let root = std::env::temp_dir().join(format!(
            "hawdb-rel-scope-{}-{}-{related}-{effect}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut db = Database::open(root.join("db")).unwrap();
        for query in [
            "CREATE (:Memory {id:'a'})",
            "CREATE (:Memory {id:'b'})",
            "CREATE (:Label {id:'l'})",
        ] {
            db.query(query).unwrap();
        }
        let kind = if related { "HAS_LABEL" } else { "OTHER" };
        if effect != "create" {
            db.query(&format!(
                "MATCH (m:Memory {{id:'b'}}),(l:Label {{id:'l'}}) CREATE (m)-[:{kind}]->(l)"
            ))
            .unwrap();
        }
        let e = db.commit_epoch().unwrap();
        let mut feed = db
            .begin_prepared_projection_owner_feed_with_scopes(
                e,
                limits(),
                budget(),
                ProjectionReadScopes {
                    node_labels: &["Memory", "Label"],
                    relationship_types: &["HAS_LABEL"],
                    ..Default::default()
                },
            )
            .unwrap();
        let mutation = match effect {
            "create" => format!(
                "MATCH (m:Memory {{id:'b'}}),(l:Label {{id:'l'}}) CREATE (m)-[:{kind}]->(l)"
            ),
            "property" => {
                format!("MATCH (m:Memory {{id:'b'}})-[r:{kind}]->(l:Label) SET r.weight = 2")
            }
            "delete" => format!("MATCH (m:Memory {{id:'b'}})-[r:{kind}]->(l:Label) DELETE r"),
            _ => unreachable!(),
        };
        db.query(&mutation).unwrap();
        feed.visit_next(budget(), |read, _| {
            assert_eq!(
                read.query_with_params(
                    "MATCH (m:Memory {id:'a'}) RETURN m.id AS id",
                    &Default::default()
                )?
                .output
                .rows
                .len(),
                1
            );
            Ok(())
        })
        .unwrap();
        let deps = feed.finish().unwrap();
        assert_eq!(deps.relationship_domain_count(), 1);
        assert!(!deps.requires_unchanged_database_epoch());
        let writer = hawdb::SearchOutOfCoreGenerationWriter::create(
            root.join("search"),
            hawdb::SearchOutOfCoreGenerationBuildOptions {
                source_graph_commit_epoch: Some(e),
                ..Default::default()
            },
        )
        .unwrap();
        // Controlled source-validation candidate, not complete mapping coverage.
        let result = db.publish_prepared_search(&deps, writer.prepare_publication().unwrap());
        if related {
            let Err(ProjectionPublicationError::Dependency(error)) = result else {
                panic!("expected relationship-scope conflict")
            };
            assert!(
                error.to_string().contains("ReadRelationshipDomain"),
                "{error}"
            );
        } else {
            result.unwrap();
        }
        drop(deps);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn eager_table_scope_keeps_one_entry_across_many_point_reads() {
    let mut db = Database::new();
    db.query_sql("CREATE TABLE rows (id BIGINT PRIMARY KEY, body TEXT)")
        .unwrap();
    db.query_sql("INSERT INTO rows (id,body) VALUES (1,'one'),(2,'two')")
        .unwrap();
    let mut config = limits();
    config.dependency_entries = NonZeroUsize::new(1).unwrap();
    let mut feed = db
        .begin_prepared_projection_owner_feed_with_scopes(
            db.commit_epoch().unwrap(),
            config,
            budget(),
            ProjectionReadScopes {
                relational_tables: &["rows"],
                ..Default::default()
            },
        )
        .unwrap();
    feed.visit_next(budget(), |read, _| {
        for id in 0..64 {
            let output =
                read.query_sql_with_params("SELECT body FROM rows WHERE id=$1", &[Value::Int(id)])?;
            assert_eq!(output.rows.len(), usize::from(id == 1 || id == 2));
        }
        Ok(())
    })
    .unwrap();
    let deps = feed.finish().unwrap();
    assert_eq!(deps.relational_table_domain_count(), 1);
    assert_eq!(deps.exact_relational_row_count(), 0);
    assert!(!deps.requires_unchanged_database_epoch());
}

#[test]
fn eager_label_scope_keeps_one_entry_across_many_scalar_index_reads() {
    let mut db = Database::new();
    db.query("CREATE (:Thread {id:'a'})").unwrap();
    db.query("CREATE INDEX ON :Thread(id)").unwrap();
    let mut config = limits();
    config.dependency_entries = NonZeroUsize::new(1).unwrap();
    let mut feed = db
        .begin_prepared_projection_owner_feed_with_scopes(
            db.commit_epoch().unwrap(),
            config,
            budget(),
            ProjectionReadScopes {
                node_labels: &["Thread"],
                ..Default::default()
            },
        )
        .unwrap();
    feed.visit_next(budget(), |read, _| {
        for id in ["a", "missing"].into_iter().cycle().take(64) {
            let output = read.query_with_params(
                "MATCH (t:Thread) WHERE t.id=$id RETURN t.id AS id",
                &std::collections::BTreeMap::from([("id".into(), Value::String(id.into()))]),
            )?;
            assert_eq!(output.output.rows.len(), usize::from(id == "a"));
        }
        Ok(())
    })
    .unwrap();
    let deps = feed.finish().unwrap();
    assert_eq!(deps.node_domain_count(), 1);
    assert_eq!(deps.exact_node_property_count(), 0);
    assert!(!deps.requires_unchanged_database_epoch());
}

#[test]
fn scope_constructor_refuses_unknown_tables_empty_names_and_unadmitted_input() {
    let db = Database::new();
    for scopes in [
        ProjectionReadScopes {
            relational_tables: &["missing"],
            ..Default::default()
        },
        ProjectionReadScopes {
            relationship_types: &[""],
            ..Default::default()
        },
    ] {
        assert!(db
            .begin_prepared_projection_owner_feed_with_scopes(
                db.commit_epoch().unwrap(),
                limits(),
                budget(),
                scopes
            )
            .is_err());
    }
    let mut config = limits();
    config.dependency_entries = NonZeroUsize::new(1).unwrap();
    assert!(db
        .begin_prepared_projection_owner_feed_with_scopes(
            db.commit_epoch().unwrap(),
            config,
            budget(),
            ProjectionReadScopes {
                node_labels: &["Memory", "Thread"],
                ..Default::default()
            }
        )
        .is_err());
    config = limits();
    config.dependency_bytes = NonZeroUsize::new(1).unwrap();
    assert!(db
        .begin_prepared_projection_owner_feed_with_scopes(
            db.commit_epoch().unwrap(),
            config,
            budget(),
            ProjectionReadScopes {
                relationship_types: &["HAS_LABEL"],
                ..Default::default()
            }
        )
        .is_err());
}
