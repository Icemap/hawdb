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
use crate::{
    HostScorer, HostScorerBatch, HostScorerDescriptor, MissingScoringFeature,
    OwnedHostScoringRequest, OwnedSqlQueryRequest, QueryRequest, ScoreFeature, ScoringCombination,
    ScoringProgram, ScoringRequest, ScoringSpec, ScoringTerm, SqlQueryRequest,
};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicUsize, Ordering};

const QUERY: &str =
    "MATCH (p:Probe) WHERE p.kind = $kind RETURN p.id AS id, p.score AS score ORDER BY id";

struct Fixture {
    engine: HawDBTokioEmbedded,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hawdb-owned-request-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        let config = crate::DatabaseConfig {
            runtime_capabilities: crate::RuntimeCapabilities::default().with(
                hawdb_core::RuntimeCapability::AccessControl,
                cfg!(feature = "acl"),
            ),
            ..crate::DatabaseConfig::default()
        };
        let engine = HawDBTokioEmbedded::open_owned(
            HawDBEmbeddedOpenOptions::new(&path).with_config(config),
        )
        .unwrap();
        engine.with_embedded_mut(|embedded| {
            let database = embedded.database_mut();
            database
                .query("CREATE (:Probe {id: 'a', kind: 'x', score: 1.0, space_id: 'a'})")
                .unwrap();
            database
                .query("CREATE (:Probe {id: 'b', kind: 'x', score: 9.0, space_id: 'b'})")
                .unwrap();
            database
                .query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY)")
                .unwrap();
            database
                .query_sql("INSERT INTO records (id) VALUES (1), (2)")
                .unwrap();
        });
        Self { engine, path }
    }
    fn close(self) {
        drop(self.engine);
        std::fs::remove_dir_all(self.path).unwrap();
    }
}

fn request() -> OwnedQueryRequest {
    OwnedQueryRequest::new(QUERY)
        .with_params(BTreeMap::from([("kind".into(), Value::String("x".into()))]))
        .with_output_limits(QueryStreamOptions {
            max_rows: Some(2),
            max_payload_bytes: Some(8_192),
        })
}
fn scoring() -> ScoringRequest {
    ScoringRequest::new(
        ScoringProgram::new(
            ScoringCombination::WeightedSum,
            MissingScoringFeature::Reject,
            ScoringSpec {
                terms: vec![ScoringTerm {
                    feature: ScoreFeature::SearchScore,
                    weight: 1.0,
                }],
                decay: vec![],
            },
        )
        .unwrap(),
        "score",
        1,
    )
    .unwrap()
}

#[test]
fn owned_requests_preserve_scoring_authorization_and_stream_caps() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<OwnedQueryRequest>();
    send_sync::<OwnedSqlQueryRequest>();
    let fixture = Fixture::new();
    let engine = &fixture.engine;
    engine
        .runtime()
        .block_on(async {
            let output = engine
                .query_request(request().with_scoring(scoring()))
                .await
                .unwrap();
            assert_eq!(output.rows[0]["id"], Value::String("b".into()));
            let access = crate::QueryAccessControlContext::visibility_scope(42, "space_id", "a");
            let result = engine
                .query_request(
                    request()
                        .with_scoring(scoring())
                        .with_access_control(access),
                )
                .await;
            #[cfg(feature = "acl")]
            assert_eq!(result.unwrap().rows[0]["id"], Value::String("a".into()));
            #[cfg(not(feature = "acl"))]
            assert!(matches!(
                result,
                Err(HawDBTokioEmbeddedError::Task(TokioTaskError::Operation(
                    HawDBError::CapabilityUnavailable { .. }
                )))
            ));
            let mut stream = engine
                .query_request_stream(request().with_scoring(scoring()))
                .await
                .unwrap();
            let rows = stream.next_batch().await.unwrap().unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["id"], Value::String("b".into()));
            assert!(stream.next_batch().await.unwrap().is_none());
            assert!(stream.report().is_some());
            let limits = QueryStreamOptions {
                max_rows: Some(0),
                max_payload_bytes: Some(8_192),
            };
            assert!(engine
                .query_request(request().with_output_limits(limits))
                .await
                .is_err());
            let mut stream = engine
                .query_request_stream(request().with_output_limits(limits))
                .await
                .unwrap();
            assert!(stream.next_batch().await.is_err());
            assert!(stream.report().is_none());
            let stopped = RuntimeTaskContext::default();
            stopped.cancellation().cancel();
            assert!(engine
                .query_request(request().with_task_context(stopped))
                .await
                .is_err());
        })
        .unwrap();
    assert_eq!(engine.runtime_snapshot().admitted_memory_bytes, 0);
    fixture.close();
}

struct Scorer {
    calls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl Drop for Scorer {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
impl HostScorer for Scorer {
    fn descriptor(&self) -> HostScorerDescriptor<'_> {
        HostScorerDescriptor::new("owned", "v1", NonZeroU64::MIN).unwrap()
    }
    fn score_batch(&mut self, batch: HostScorerBatch<'_>, scores: &mut [f64]) -> crate::Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        for (row, score) in batch.features.iter().zip(scores) {
            batch.checkpoint()?;
            *score = row.search_score().unwrap();
        }
        Ok(())
    }
}

#[test]
fn owned_host_callback_runs_once_and_releases_on_success_and_cancellation() {
    let fixture = Fixture::new();
    let engine = &fixture.engine;
    let calls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    engine
        .runtime()
        .block_on(async {
            for cancelled in [false, true] {
                let host = OwnedHostScoringRequest::new(
                    Scorer {
                        calls: calls.clone(),
                        drops: drops.clone(),
                    },
                    "score",
                    NonZeroUsize::new(2).unwrap(),
                    1,
                )
                .unwrap();
                let context = RuntimeTaskContext::default();
                if cancelled {
                    context.cancellation().cancel();
                }
                let result = engine
                    .query_request(request().with_host_scoring(host).with_task_context(context))
                    .await;
                if cancelled {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.unwrap().rows[0]["id"], Value::String("b".into()));
                }
            }
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    fixture.close();
}

#[test]
fn admitted_sql_reuses_requests_and_rejects_returning_before_commit() {
    let fixture = Fixture::new();
    let engine = &fixture.engine;
    let cache_stats = || {
        engine.with_embedded(|embedded| {
            embedded
                .database()
                .relational_plan_template_cache_stats()
                .unwrap()
        })
    };
    engine
        .runtime()
        .block_on(async {
            let before = cache_stats();
            const SQL: &str = "SELECT id FROM records WHERE id >= $1 ORDER BY id";
            let output = engine
                .query_sql_request(OwnedSqlQueryRequest::new(SQL).with_params(vec![Value::Int(1)]))
                .await
                .unwrap();
            assert_eq!(output.rows.len(), 2);
            let first = cache_stats();
            assert_eq!(first.misses, before.misses + 1);
            assert_eq!(first.hits, before.hits, "execution must reuse preparation");
            let output = engine
                .query_sql_request(OwnedSqlQueryRequest::new(SQL).with_params(vec![Value::Int(2)]))
                .await
                .unwrap();
            assert_eq!(output.rows.len(), 1);
            assert_eq!(output.rows[0]["id"], Value::Int(2));
            let second = cache_stats();
            assert_eq!(second.misses, first.misses);
            assert_eq!(second.hits, first.hits + 1);
            let refused = engine
                .query_sql_request(
                    OwnedSqlQueryRequest::new("INSERT INTO records (id) VALUES ($1) RETURNING id")
                        .with_params(vec![Value::Int(3)])
                        .with_output_limits(QueryStreamOptions {
                            max_rows: Some(0),
                            max_payload_bytes: None,
                        }),
                )
                .await;
            assert!(refused.is_err());
            assert!(engine
                .query_sql_request(OwnedSqlQueryRequest::new(
                    "SELECT id FROM records WHERE id = 3"
                ))
                .await
                .unwrap()
                .rows
                .is_empty());
            let stopped = RuntimeTaskContext::default();
            stopped.cancellation().cancel();
            assert!(engine
                .query_sql_request(
                    OwnedSqlQueryRequest::new("SELECT id FROM records").with_task_context(stopped)
                )
                .await
                .is_err());
        })
        .unwrap();
    engine.with_embedded_mut(|embedded| {
        assert_eq!(
            embedded
                .query_sql_request_admitted(SqlQueryRequest::new("SELECT id FROM records"))
                .unwrap()
                .rows
                .len(),
            2
        );
        embedded.begin_transaction().unwrap();
    });
    engine
        .runtime()
        .block_on(async {
            assert!(engine.query_request(request()).await.is_err());
            assert!(engine
                .query_sql_request(OwnedSqlQueryRequest::new("SELECT id FROM records"))
                .await
                .is_err());
        })
        .unwrap();
    engine.with_embedded_mut(|embedded| embedded.rollback_transaction().unwrap());
    assert_eq!(engine.runtime_snapshot().admitted_memory_bytes, 0);
    fixture.close();
}

#[test]
fn owning_a_request_preserves_the_synchronous_contract() {
    let fixture = Fixture::new();
    let query = request().with_scoring(scoring());
    fixture.engine.with_embedded_mut(|embedded| {
        let owned = embedded
            .database_mut()
            .query_request(query.as_request())
            .unwrap();
        assert_eq!(owned.rows[0]["id"], Value::String("b".into()));
        let params = BTreeMap::from([("kind".into(), Value::String("x".into()))]);
        let score = scoring();
        let borrowed = embedded
            .database_mut()
            .query_request(
                QueryRequest::new(QUERY)
                    .with_params(&params)
                    .with_scoring(&score),
            )
            .unwrap();
        assert_eq!(borrowed.rows, owned.rows);
    });
    fixture.close();
}
