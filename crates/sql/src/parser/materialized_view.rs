// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;

pub(super) fn lower_create(create: &sqlparser::ast::CreateView) -> Result<SqlStatement> {
    if !create.materialized {
        return Err(HawDBError::Semantic(
            "only MATERIALIZED VIEW is supported".into(),
        ));
    }
    reject_unsupported_clauses(
        "CREATE MATERIALIZED VIEW",
        &[
            ("OR ALTER", create.or_alter),
            ("OR REPLACE", create.or_replace),
            ("SECURE", create.secure),
            ("COPY GRANTS", create.copy_grants),
            ("column list", !create.columns.is_empty()),
            (
                "options",
                !matches!(create.options, sqlparser::ast::CreateTableOptions::None),
            ),
            ("CLUSTER BY", !create.cluster_by.is_empty()),
            ("COMMENT", create.comment.is_some()),
            ("WITH NO SCHEMA BINDING", create.with_no_schema_binding),
            ("TEMPORARY", create.temporary),
            ("TO", create.to.is_some()),
            ("parameters", create.params.is_some()),
        ],
    )?;
    let SqlStatement::Select(query) = lower_select_statement(&create.query)? else {
        unreachable!("SELECT lowering returns a SELECT statement")
    };
    Ok(SqlStatement::CreateMaterializedView(
        CreateMaterializedViewStatement {
            name: lower_table_name(&create.name)?,
            if_not_exists: create.if_not_exists,
            query: Box::new(query),
        },
    ))
}

pub(super) fn lower_drop(statement: &ParserStatement) -> Result<SqlStatement> {
    let ParserStatement::Drop {
        names,
        if_exists,
        cascade,
        purge,
        temporary,
        table,
        ..
    } = statement
    else {
        unreachable!("DROP lowering requires a DROP statement")
    };
    reject_unsupported_clauses(
        "DROP MATERIALIZED VIEW",
        &[
            ("CASCADE", *cascade),
            ("PURGE", *purge),
            ("TEMPORARY", *temporary),
            ("table", table.is_some()),
            ("multiple names", names.len() != 1),
        ],
    )?;
    Ok(SqlStatement::DropMaterializedView(
        DropMaterializedViewStatement {
            name: lower_table_name(&names[0])?,
            if_exists: *if_exists,
        },
    ))
}
