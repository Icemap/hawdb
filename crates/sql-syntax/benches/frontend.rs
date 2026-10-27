// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use std::hint::black_box;
use std::time::Instant;

use hawdb_sql_syntax::parse_postgres_statement;

fn main() {
    let queries = [
        ("lookup", "SELECT id FROM messages WHERE thread_id = $1 ORDER BY id LIMIT 20".into(), 5_000, true),
        ("aggregate", "SELECT thread_id, count(*) AS n FROM messages GROUP BY thread_id HAVING count(*) > 0".into(), 5_000, true),
        ("join", "SELECT a.id, b.name FROM nodes a LEFT JOIN names b ON a.id = b.id WHERE b.name IS NOT NULL".into(), 5_000, true),
        ("pgq", "SELECT g.id FROM GRAPH_TABLE (knowledge MATCH (n IS Thing) COLUMNS (n.id)) g WHERE g.id = $1".into(), 5_000, true),
        ("create", "CREATE PROPERTY GRAPH knowledge VERTEX TABLES (nodes KEY (id) PROPERTIES ALL COLUMNS)".into(), 5_000, true),
        ("minimal", "SELECT * FROM t".into(), 5_000, true),
        ("incomplete_pgq", "SELECT * FROM GRAPH_TABLE (g MATCH (n) COLUMNS (n.id".into(), 5_000, false),
        ("wide_projection", format!("SELECT {} FROM t", (0..500).map(|i| format!("c{i}")).collect::<Vec<_>>().join(",")), 100, true),
        ("many_joins", format!("SELECT a.id FROM a {}", (0..64).map(|i| format!("LEFT JOIN t{i} ON a.id = t{i}.id ")).collect::<String>()), 100, true),
        ("long_error", format!("SELECT * FROM t AS a {}", "x".repeat(4 * 1024 * 1024)), 10, false),
    ];
    // Initialize the grammar and validate fixture outcomes outside measurement.
    for (name, sql, _, accepted) in &queries {
        assert_eq!(parse_postgres_statement(sql).is_ok(), *accepted, "{name}");
    }
    println!("case,sample,iterations,nanoseconds");
    if std::env::args().any(|arg| arg == "--shapes") {
        for (name, sql, repetitions, _) in &queries {
            for sample in 0..9 {
                let start = Instant::now();
                for _ in 0..*repetitions {
                    drop(black_box(parse_postgres_statement(black_box(sql))));
                }
                println!(
                    "{name},{sample},{repetitions},{}",
                    start.elapsed().as_nanos()
                );
            }
        }
    } else {
        for sample in 0..9 {
            let start = Instant::now();
            for _ in 0..5_000 {
                for (_, sql, _, _) in &queries[..5] {
                    drop(black_box(parse_postgres_statement(black_box(sql))));
                }
            }
            println!("mixed,{sample},25000,{}", start.elapsed().as_nanos());
        }
    }
}
