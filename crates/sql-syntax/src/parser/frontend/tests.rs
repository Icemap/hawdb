// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use crate::{
    parse_graph_table, parse_postgres_select, parse_postgres_statement, PostgresFromItemSyntax,
    Span, SyntaxErrorCode,
};

#[test]
fn graph_table_prefix_cannot_backtrack_into_a_relation() {
    for sql in [
        "SELECT * FROM GRAPH_TABLE",
        "SELECT * FROM GRAPH_TABLE (g MATCH (n) COLUMNS ())",
        "SELECT * FROM GRAPH_TABLE (g MATCH (n) COLUMNS (n.id)",
        "SELECT * FROM source JOIN GRAPH_TABLE ON source.id = 1",
    ] {
        assert!(parse_postgres_select(sql).is_err(), "{sql}");
        assert!(parse_postgres_statement(sql).is_err(), "{sql}");
    }
    for name in ["\"GRAPH_TABLE\"", "graph_table_archive"] {
        let sql = format!("SELECT * FROM {name}");
        let select = parse_postgres_select(&sql).unwrap();
        assert!(matches!(
            select.from[0].relation,
            PostgresFromItemSyntax::Relation(_)
        ));
    }
}

#[test]
fn composed_sql_pgq_keeps_utf8_spans_comments_and_independent_parses() {
    let sql = "SELECT g.id FROM /* nested /* comment */ */ GRAPH_TABLE (\"\u{77e5}\u{8b58}\" MATCH (n) COLUMNS (n.id)) AS g";
    let select = parse_postgres_select(sql).unwrap();
    let PostgresFromItemSyntax::GraphTable(table) = &select.from[0].relation else {
        panic!("PGQ alternative")
    };
    assert_eq!(
        &sql[table.graph.span.start..table.graph.span.end],
        "\"\u{77e5}\u{8b58}\""
    );
    assert_eq!(
        &sql[table.span.start..table.span.end],
        "GRAPH_TABLE (\"\u{77e5}\u{8b58}\" MATCH (n) COLUMNS (n.id)) AS g"
    );
    let bad = format!("{sql} WHERE");
    let error = parse_postgres_select(&bad).unwrap_err();
    assert_eq!(error.code, SyntaxErrorCode::UnexpectedEnd);
    assert_eq!(error.span, Span::new(bad.len(), bad.len()));
    assert_eq!(parse_postgres_select(sql).unwrap(), select);
    assert!(parse_graph_table("GRAPH_TABLE (g MATCH (n) COLUMNS (n.id)); SELECT 1").is_err());
}
