//! The agent runner. Patterns 2, 3 and 7.
//!
//! Every agent is a module with a packet in and an output out (doc 10 section
//! 2). What surrounds that is identical across all nine, so it is built once
//! here:
//!
//!   - validate the packet on entry, and fail before spending anything if it is
//!     malformed (doc 03 section 6: missing doctrine or profile is a hard
//!     failure);
//!   - drive the agent's state machine;
//!   - validate the output against its schema, and on a violation retry once
//!     with the violation attached, which is what every agent spec prescribes;
//!   - write the Step with its packet, output, model call and failure;
//!   - emit the events, in the same transaction as the Step.
//!
//! The retry rule is the one worth stating plainly: the agent is told what it
//! got wrong, in the words of the schema that rejected it. A retry that just
//! says "try again" spends a call to get the same answer.

use async_trait::async_trait;
use serde_json::{Value, json};
use tessera_providers::{Completion, CompletionRequest, ModelProvider, ResolvedPolicy};
use tessera_schema::Registry;
use tessera_store::{NewEvent, Provenance, Source, Store, new_id, now_iso8601};

use crate::failure::{Failure, Recovery};
use crate::state::Machine;

/// One model call, in the shape `Step.model_call` expects (doc 01 section 6.2).
/// The prompt itself is stored by hash in the blob store, so the audit trail can
/// reproduce the call and the database stays small.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelCallRecord {
    pub provider: String,
    pub model: String,
    pub prompt_hash: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub stage: String,
}

/// What an agent gets while it runs.
pub struct AgentContext<'a> {
    pub registry: &'a Registry,
    pub provider: &'a dyn ModelProvider,
    pub run_id: String,
    pub card_id: Option<String>,
    pub board_id: Option<String>,
    /// Drives the states from the agent's spec.
    pub machine: Machine,
    /// Filled in as calls are made; folded into the Step and into `Run.cost`.
    pub model_calls: Vec<ModelCallRecord>,
    /// Set on the second attempt. Doc 03 section 10: "retry once with violation
    /// detail". The agent puts this in its prompt.
    pub last_violations: Option<Value>,
    /// Provenance for every event this run emits. `test` under the eval harness,
    /// so policy hooks do not fire (doc 02 section 10.1, doc 10 section 5).
    pub source: Source,
    /// Every stage resolved to a concrete model, snapshotted at run start.
    /// Doc 03 section 8.3.
    pub policy: ResolvedPolicy,
}

impl AgentContext<'_> {
    /// The concrete model a stage resolved to.
    ///
    /// A stage the policy did not resolve is a harness bug rather than a user
    /// problem, so this falls back to whatever the provider considers current
    /// instead of failing the card at the point of use. The resolver already
    /// refused to start a run whose required stages had no key (doc 03 section
    /// 8.3 `policy_unresolvable`).
    pub fn model_for(&self, stage: &str) -> String {
        self.policy
            .get(stage)
            .map(|s| s.model.clone())
            .unwrap_or_else(|| "claude-sonnet-5".to_string())
    }

    /// The alias name a stage resolved to, for `produced_by.model_alias`.
    pub fn alias_for(&self, stage: &str) -> String {
        self.policy
            .get(stage)
            .map(|s| s.alias.clone())
            .unwrap_or_else(|| "medium".to_string())
    }

    /// Make a model call and record it. Every call an agent makes goes through
    /// here, so nothing reaches a provider without landing in the audit trail.
    pub async fn call(&mut self, request: &CompletionRequest) -> Result<Completion, Failure> {
        let completion = self.provider.complete(request).await?;
        self.model_calls.push(ModelCallRecord {
            provider: completion.provider.clone(),
            model: completion.model.clone(),
            prompt_hash: request.prompt_hash(),
            input_tokens: completion.usage.input_tokens,
            output_tokens: completion.usage.output_tokens,
            latency_ms: completion.latency_ms,
            stage: request.stage.clone(),
        });
        Ok(completion)
    }

    /// The text an agent appends to its prompt on a retry.
    pub fn violation_notice(&self) -> Option<String> {
        let v = self.last_violations.as_ref()?;
        Some(format!(
            "Your previous response failed validation. Fix exactly these problems and return the whole object again:\n{}",
            serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
        ))
    }
}

#[async_trait]
pub trait Agent: Send + Sync {
    /// Matches `Step.agent_id` in doc 01 section 6.2.
    fn id(&self) -> &str;

    /// Registry ids for the two boundaries this agent sits between.
    fn packet_schema(&self) -> &'static str;
    fn output_schema(&self) -> &'static str;

    /// The state sequence from this agent's spec.
    fn states(&self) -> &'static [&'static str];

    /// Doc 07 section B6: the Verifier's deterministic stages never retry.
    fn allows_retry(&self) -> bool {
        true
    }

    /// The event this agent emits on success, for instance `card.routed.v1`.
    fn completion_event(&self) -> Option<&'static str> {
        None
    }

    /// The payload for that event. Each agent spec declares its own fields
    /// (doc 03 section 7 and the equivalent in the others), so an agent that
    /// emits an event says what goes in it rather than leaving the harness to
    /// guess from the shape of its output.
    fn completion_payload(&self, output: &Value) -> Value {
        let _ = output;
        Value::Null
    }

    async fn execute(&self, ctx: &mut AgentContext<'_>, packet: &Value) -> Result<Value, Failure>;
}

#[derive(Debug)]
pub struct AgentOutcome {
    pub output: Value,
    pub step_id: String,
    pub model_calls: Vec<ModelCallRecord>,
    /// States actually entered, in order. The phase 2 acceptance test asserts
    /// a mock run walked every one.
    pub visited: Vec<String>,
    pub attempts: u8,
}

pub struct RunAgent<'a> {
    pub registry: &'a Registry,
    pub provider: &'a dyn ModelProvider,
    pub run_id: String,
    pub card_id: Option<String>,
    pub board_id: Option<String>,
    pub sequence: i64,
    pub source: Source,
    pub policy: ResolvedPolicy,
}

/// Validate, run, validate, retry once, record.
pub async fn run_agent(
    agent: &dyn Agent,
    store: &mut Store,
    cfg: RunAgent<'_>,
    packet: Value,
) -> Result<AgentOutcome, Failure> {
    let started_at = now_iso8601();

    // Entry guard. Doc 12 operating principle 1: validate at every boundary. A
    // malformed packet is the harness's bug, so it fails before any spend.
    if let Err(e) = cfg.registry.validate(agent.packet_schema(), &packet) {
        let failure = Failure {
            kind: "packet_invalid".into(),
            detail: e.to_string(),
            recovery: Recovery::Failed,
            evidence: None,
            recoverable: false,
        };
        record_step(
            store,
            agent,
            &cfg,
            StepRecord {
                packet: &packet,
                output: None,
                model_calls: &[],
                failure: Some(&failure),
                started_at: &started_at,
            },
        )?;
        emit_violation(store, agent, &cfg, &e)?;
        return Err(failure);
    }

    let mut last_violations: Option<Value> = None;
    let mut model_calls: Vec<ModelCallRecord> = Vec::new();
    let mut attempts: u8 = 0;

    loop {
        attempts += 1;

        let mut ctx = AgentContext {
            registry: cfg.registry,
            provider: cfg.provider,
            run_id: cfg.run_id.clone(),
            card_id: cfg.card_id.clone(),
            board_id: cfg.board_id.clone(),
            machine: {
                let m = Machine::new(agent.id(), agent.states());
                if agent.allows_retry() { m } else { m.forbid_retry() }
            },
            model_calls: Vec::new(),
            last_violations: last_violations.clone(),
            source: cfg.source,
            policy: cfg.policy.clone(),
        };

        let result = agent.execute(&mut ctx, &packet).await;
        let this_attempt = std::mem::take(&mut ctx.model_calls);
        // Emit before deciding the outcome, so a violation event always follows
        // the call that produced it rather than preceding it.
        emit_model_calls(store, agent, &cfg, &this_attempt)?;
        model_calls.extend(this_attempt);

        match result {
            Ok(output) => match cfg.registry.validate(agent.output_schema(), &output) {
                Ok(()) => {
                    let visited: Vec<String> =
                        ctx.machine.visited().iter().map(|s| (*s).to_string()).collect();
                    let step_id = record_step(
                        store,
                        agent,
                        &cfg,
                        StepRecord {
                            packet: &packet,
                            output: Some(&output),
                            model_calls: &model_calls,
                            failure: None,
                            started_at: &started_at,
                        },
                    )?;
                    if let Some(event) = agent.completion_event() {
                        store.append(with_ids(
                            NewEvent::new(
                                event,
                                completion_payload(agent, &cfg, &output),
                                Provenance::agent(agent.id(), cfg.run_id.clone()).with_source(cfg.source),
                            ),
                            &cfg,
                        ))?;
                    }
                    return Ok(AgentOutcome {
                        output,
                        step_id,
                        model_calls,
                        visited,
                        attempts,
                    });
                }
                Err(e) => {
                    emit_violation(store, agent, &cfg, &e)?;
                    // One retry, with the violation attached. Every agent spec.
                    if attempts == 1 && agent.allows_retry() {
                        last_violations = match &e {
                            tessera_schema::SchemaError::Invalid { violations, .. } => {
                                serde_json::to_value(violations).ok()
                            }
                            _ => None,
                        };
                        continue;
                    }
                    let failure: Failure = e.into();
                    let failure = Failure {
                        recovery: Recovery::Failed,
                        recoverable: false,
                        ..failure
                    };
                    record_step(
                        store,
                        agent,
                        &cfg,
                        StepRecord {
                            packet: &packet,
                            output: None,
                            model_calls: &model_calls,
                            failure: Some(&failure),
                            started_at: &started_at,
                        },
                    )?;
                    return Err(failure);
                }
            },
            Err(failure) => {
                // A recoverable failure the agent reported itself gets the same
                // single retry, so a provider blip does not lose the card.
                if attempts == 1 && failure.recoverable && agent.allows_retry() {
                    continue;
                }
                let failure = Failure {
                    recovery: Recovery::Failed,
                    recoverable: false,
                    ..failure
                };
                record_step(
                    store,
                    agent,
                    &cfg,
                    StepRecord {
                        packet: &packet,
                        output: None,
                        model_calls: &model_calls,
                        failure: Some(&failure),
                        started_at: &started_at,
                    },
                )?;
                return Err(failure);
            }
        }
    }
}

fn with_ids(mut ev: NewEvent, cfg: &RunAgent<'_>) -> NewEvent {
    if let Some(board) = &cfg.board_id {
        ev = ev.on_board(board.clone());
    }
    if let Some(card) = &cfg.card_id {
        ev = ev.on_card(card.clone());
    }
    ev
}

fn completion_payload(agent: &dyn Agent, cfg: &RunAgent<'_>, output: &Value) -> Value {
    let mut payload = match agent.completion_payload(output) {
        Value::Object(map) => Value::Object(map),
        // An agent that declared an event but no payload gets the identifying
        // fields only. The Step already holds the full output, so the event does
        // not need to repeat it.
        _ => json!({}),
    };
    payload["agent_id"] = json!(agent.id());
    payload["run_id"] = json!(cfg.run_id);
    if let Some(card) = &cfg.card_id {
        payload["card_id"] = json!(card);
    }
    payload
}

/// What one attempt produced, in the shape the Step row wants.
struct StepRecord<'a> {
    packet: &'a Value,
    output: Option<&'a Value>,
    model_calls: &'a [ModelCallRecord],
    failure: Option<&'a Failure>,
    started_at: &'a str,
}

fn record_step(
    store: &mut Store,
    agent: &dyn Agent,
    cfg: &RunAgent<'_>,
    record: StepRecord<'_>,
) -> Result<String, Failure> {
    let StepRecord {
        packet,
        output,
        model_calls,
        failure,
        started_at,
    } = record;
    let step_id = new_id();
    let status = match failure {
        None => "done",
        Some(f) if f.recoverable => "retried",
        Some(_) => "failed",
    };
    let model_call = model_calls
        .last()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| Failure {
            kind: "store".into(),
            detail: e.to_string(),
            recovery: Recovery::Failed,
            evidence: None,
            recoverable: false,
        })?;

    store
        .conn()
        .execute(
            "INSERT INTO step (id, run_id, agent_id, sequence, task_packet, output, model_call,
                               status, failure, started_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                step_id,
                cfg.run_id,
                agent.id(),
                cfg.sequence,
                packet.to_string(),
                output.map(ToString::to_string),
                model_call,
                status,
                failure.map(|f| serde_json::to_string(f).unwrap_or_default()),
                started_at,
                now_iso8601(),
            ],
        )
        .map_err(|e| Failure {
            kind: "store".into(),
            detail: e.to_string(),
            recovery: Recovery::Failed,
            evidence: None,
            recoverable: false,
        })?;

    Ok(step_id)
}

fn emit_model_calls(
    store: &mut Store,
    agent: &dyn Agent,
    cfg: &RunAgent<'_>,
    calls: &[ModelCallRecord],
) -> Result<(), Failure> {
    for call in calls {
        store.append(with_ids(
            NewEvent::new(
                "model.call.v1",
                json!({
                    "stage": call.stage,
                    "provider": call.provider,
                    "model": call.model,
                    "prompt_hash": call.prompt_hash,
                    "input_tokens": call.input_tokens,
                    "output_tokens": call.output_tokens,
                    "latency_ms": call.latency_ms,
                }),
                Provenance::agent(agent.id(), cfg.run_id.clone()).with_source(cfg.source),
            ),
            cfg,
        ))?;
    }
    Ok(())
}

fn emit_violation(
    store: &mut Store,
    agent: &dyn Agent,
    cfg: &RunAgent<'_>,
    error: &tessera_schema::SchemaError,
) -> Result<(), Failure> {
    let violations = match error {
        tessera_schema::SchemaError::Invalid { violations, .. } => {
            serde_json::to_value(violations).unwrap_or(Value::Null)
        }
        other => json!(other.to_string()),
    };
    store.append(with_ids(
        NewEvent::new(
            "schema.violation.v1",
            json!({ "agent_id": agent.id(), "violations": violations }),
            Provenance::agent(agent.id(), cfg.run_id.clone()).with_source(cfg.source),
        ),
        cfg,
    ))?;
    Ok(())
}
