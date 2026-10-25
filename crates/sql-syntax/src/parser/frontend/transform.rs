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

use super::{Output, Transformer, Value};
use crate::peg::NodeId;
use crate::*;

macro_rules! take_native {
    ($transformer:expr, $node:expr, $name:literal, $variant:ident) => {{
        let child = $transformer.tree.required($node, $name);
        let Value::$variant(value) = $transformer.tree.take(child) else {
            unreachable!("native terminal type matches its grammar rule")
        };
        value
    }};
}

impl Transformer<'_> {
    pub(super) fn output(&mut self, root: &str) -> Output {
        let mut node = self.tree.nodes[self.tree.root].children[0];
        if root == "SqlInput" {
            node = self.tree.nodes[node].children[0];
        }
        match self.tree.nodes[node].name {
            Some("Select") => Output::Select(Box::new(self.select(node))),
            Some("CreatePropertyGraph") => Output::Create(Box::new(self.create(node))),
            Some("GraphTable") => Output::GraphTable(Box::new(self.graph_table(node))),
            _ => unreachable!("entry grammar has a typed statement transformer"),
        }
    }

    fn select(&mut self, node: NodeId) -> PostgresSelectSyntax {
        let distinct = self
            .tree
            .child(node, "Quantifier")
            .is_some_and(|quantifier| self.tree.child(quantifier, "Distinct").is_some());
        let projection = take_native!(self, node, "Projection", Projection);
        let from = self.tree.required(node, "From");
        let from = self.tree.nodes[from]
            .children
            .clone()
            .into_iter()
            .map(|source| {
                let relation = self.transform_from_item(self.tree.required(source, "FromItem"));
                let joins = self.tree.nodes[source]
                    .children
                    .clone()
                    .into_iter()
                    .filter(|child| self.tree.nodes[*child].name == Some("Join"))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|join| self.join(join))
                    .collect();
                PostgresFromSyntax {
                    relation,
                    joins,
                    span: self.tree.span(source, self.tokens),
                }
            })
            .collect();
        let selection = self.clause(node, "Where");
        let group_by = self
            .tree
            .child(node, "Group")
            .map_or_else(Vec::new, |group| {
                take_native!(self, group, "GroupExpressions", Expressions)
            });
        let having = self.clause(node, "Having");
        let order_by = self
            .tree
            .child(node, "Order")
            .map_or_else(Vec::new, |order| {
                take_native!(self, order, "OrderItems", OrderItems)
            });
        let limit = self.clause(node, "Limit");
        let offset = self.clause(node, "Offset");
        let locking = self
            .tree
            .child(node, "Locking")
            .map(|locking| LockingClauseSyntax {
                strength: if self.tree.child(locking, "Share").is_some() {
                    LockStrengthSyntax::Share
                } else {
                    LockStrengthSyntax::Update
                },
                span: self.tree.span(locking, self.tokens),
            });
        PostgresSelectSyntax {
            distinct,
            projection,
            from,
            selection,
            group_by,
            having,
            order_by,
            limit,
            offset,
            locking,
            span: self.tree.span(node, self.tokens),
        }
    }

    fn clause(&mut self, node: NodeId, name: &str) -> Option<ExpressionSyntax> {
        self.tree
            .child(node, name)
            .map(|clause| *take_native!(self, clause, "ClauseExpression", Expression))
    }

    fn transform_from_item(&mut self, node: NodeId) -> PostgresFromItemSyntax {
        let child = self.tree.nodes[node].children[0];
        if self.tree.nodes[child].name == Some("GraphTable") {
            PostgresFromItemSyntax::GraphTable(self.graph_table(child))
        } else {
            let Value::Relation(relation) = self.tree.take(child) else {
                unreachable!("Relation produces a relation AST")
            };
            PostgresFromItemSyntax::Relation(relation)
        }
    }

    fn join(&mut self, node: NodeId) -> PostgresJoinSyntax {
        let node = self.tree.nodes[node].children[0];
        let kind = match self.tree.nodes[node].name {
            Some("InnerJoin") => PostgresJoinKind::Inner,
            Some("LeftJoin") => PostgresJoinKind::Left,
            Some("CrossJoin") => PostgresJoinKind::Cross,
            _ => unreachable!("Join has three qualified alternatives"),
        };
        let relation = self.transform_from_item(self.tree.required(node, "FromItem"));
        let condition = self.tree.child(node, "JoinExpression").map(|condition| {
            let Value::Expression(expression) = self.tree.take(condition) else {
                unreachable!("JoinExpression produces an expression AST")
            };
            *expression
        });
        PostgresJoinSyntax {
            kind,
            relation,
            condition,
            span: self.tree.span(node, self.tokens),
        }
    }

    fn graph_table(&mut self, node: NodeId) -> GraphTable {
        GraphTable {
            graph: take_native!(self, node, "Name", Name),
            pattern: take_native!(self, node, "Pattern", Pattern),
            columns: take_native!(self, node, "Columns", Columns),
            alias: take_native!(self, node, "Alias", Alias),
            span: self.tree.span(node, self.tokens),
        }
    }

    fn create(&mut self, node: NodeId) -> CreatePropertyGraph {
        CreatePropertyGraph {
            temporary: self.tree.child(node, "Temporary").is_some(),
            name: take_native!(self, node, "Name", Name),
            vertex_tables: self
                .tree
                .child(node, "Vertices")
                .map_or_else(Vec::new, |vertices| {
                    take_native!(self, vertices, "VertexDefinitions", Vertices)
                }),
            edge_tables: self
                .tree
                .child(node, "Edges")
                .map_or_else(Vec::new, |edges| {
                    take_native!(self, edges, "EdgeDefinitions", Edges)
                }),
            span: self.tree.span(node, self.tokens),
        }
    }
}
