//! The schema guard. Pattern 32.
//!
//! Doc 10 principle 4: "Task packets, agent outputs, events, doctrine packs, and
//! bundles are versioned schemas validated on entry. A schema change goes through
//! a migration registry."
//!
//! Doc 12 operating principle 1 puts it more sharply: write the schemas before
//! any agent code, validate at every boundary, version from the first commit.
//! This crate is that boundary. Nothing reaches storage or an agent without
//! passing through [`Registry::validate`].

use std::collections::HashMap;
use std::sync::Arc;

use jsonschema::{Retrieve, Uri, Validator};
use serde::{Deserialize, Serialize};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// One reason an instance failed its schema.
///
/// Doc 03 section 10 and doc 06 section A10 both retry once "with the violation
/// attached", so the message has to be specific enough to put in a prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Violation {
    /// JSON pointer to the offending location in the instance.
    pub instance_path: String,
    /// JSON pointer to the keyword in the schema that rejected it.
    pub schema_path: String,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let at = if self.instance_path.is_empty() {
            "/"
        } else {
            &self.instance_path
        };
        write!(f, "{at}: {}", self.message)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("no schema registered under `{0}`")]
    Unknown(String),

    #[error("schema `{id}` failed to compile: {message}")]
    Compile { id: String, message: String },

    #[error("`{id}` rejected the instance: {}", .violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid { id: String, violations: Vec<Violation> },

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SchemaError>;

/// Serves `tessera:` references from the embedded set, so compiling a schema
/// never touches the network or the filesystem.
struct EmbeddedRetriever {
    documents: Arc<HashMap<String, Value>>,
}

impl Retrieve for EmbeddedRetriever {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> std::result::Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let key = uri.as_str();
        self.documents
            .get(key)
            .cloned()
            .ok_or_else(|| format!("no embedded schema for `{key}`").into())
    }
}

/// Every schema, compiled once at startup.
pub struct Registry {
    documents: Arc<HashMap<String, Value>>,
    validators: HashMap<String, Validator>,
}

impl Registry {
    /// Load and compile every embedded schema. Fails loudly: a schema that does
    /// not compile is a build error wearing a runtime disguise, and the guard is
    /// worthless if some of it silently did not load.
    pub fn load() -> Result<Self> {
        let mut documents = HashMap::new();
        for (path, body) in EMBEDDED {
            let doc: Value = serde_json::from_str(body).map_err(|e| SchemaError::Compile {
                id: (*path).to_string(),
                message: format!("not valid json: {e}"),
            })?;
            let id = doc
                .get("$id")
                .and_then(Value::as_str)
                .ok_or_else(|| SchemaError::Compile {
                    id: (*path).to_string(),
                    message: "every schema must carry an $id".into(),
                })?
                .to_string();
            if let Some(previous) = documents.insert(id.clone(), doc) {
                let _ = previous;
                return Err(SchemaError::Compile {
                    id,
                    message: "two schema files claim the same $id".into(),
                });
            }
        }

        let documents = Arc::new(documents);
        let mut validators = HashMap::new();
        for (id, doc) in documents.iter() {
            let validator = jsonschema::options()
                .with_retriever(EmbeddedRetriever {
                    documents: Arc::clone(&documents),
                })
                .build(doc)
                .map_err(|e| SchemaError::Compile {
                    id: id.clone(),
                    message: e.to_string(),
                })?;
            validators.insert(id.clone(), validator);
        }

        Ok(Self {
            documents,
            validators,
        })
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.validators.keys().map(String::as_str)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.validators.contains_key(id)
    }

    pub fn document(&self, id: &str) -> Option<&Value> {
        self.documents.get(id)
    }

    /// Collect every violation rather than the first, because an agent retrying
    /// after one violation only to hit the next wastes a call.
    pub fn violations(&self, id: &str, instance: &Value) -> Result<Vec<Violation>> {
        let validator = self
            .validators
            .get(id)
            .ok_or_else(|| SchemaError::Unknown(id.to_string()))?;
        Ok(validator
            .iter_errors(instance)
            .map(|e| Violation {
                instance_path: e.instance_path().to_string(),
                schema_path: e.schema_path().to_string(),
                message: e.to_string(),
            })
            .collect())
    }

    /// The boundary check. Doc 12 operating principle 1.
    pub fn validate(&self, id: &str, instance: &Value) -> Result<()> {
        let violations = self.violations(id, instance)?;
        if violations.is_empty() {
            Ok(())
        } else {
            Err(SchemaError::Invalid {
                id: id.to_string(),
                violations,
            })
        }
    }

    /// A self contained copy of a schema, with every `tessera:` reference
    /// inlined.
    ///
    /// Doc 10 section 7: structured output goes through JSON mode where the
    /// provider has it. A provider cannot resolve our reference scheme, so the
    /// schema it receives has to carry its own definitions. The registry copy
    /// stays split, because that is what keeps one definition of a ULID.
    pub fn bundled(&self, id: &str) -> Result<Value> {
        let doc = self
            .documents
            .get(id)
            .ok_or_else(|| SchemaError::Unknown(id.to_string()))?;
        jsonschema::options()
            .with_retriever(EmbeddedRetriever {
                documents: Arc::clone(&self.documents),
            })
            .bundle(doc)
            .map_err(|e| SchemaError::Compile {
                id: id.to_string(),
                message: format!("could not bundle: {e}"),
            })
    }

    pub fn is_valid(&self, id: &str, instance: &Value) -> Result<bool> {
        Ok(self.violations(id, instance)?.is_empty())
    }

    /// Validate, then deserialise. The order matters: a serde error says which
    /// Rust field disagreed, a schema violation says which contract the agent
    /// broke, and the second is what goes back to the model on a retry.
    pub fn parse<T: serde::de::DeserializeOwned>(&self, id: &str, instance: &Value) -> Result<T> {
        self.validate(id, instance)?;
        Ok(serde_json::from_value(instance.clone())?)
    }
}

/// Schema ids, so a typo is a compile error rather than an unknown-schema error
/// at the boundary the guard exists to protect.
pub mod ids {
    pub const EVENT_ENVELOPE: &str = "tessera:event/envelope.v1";
    pub const COMMON: &str = "tessera:entity/common.v1";
    pub const VISUAL: &str = "tessera:entity/visual.v1";
    pub const STRUCTURED_SUMMARY: &str = "tessera:entity/structured-summary.v1";
    pub const DOCTRINE_PACK: &str = "tessera:pack/doctrine-pack.v1";
    pub const BUNDLE_MANIFEST: &str = "tessera:bundle/manifest.v1";

    pub const PACKET_ROUTER: &str = "tessera:packet/router.v1";
    pub const PACKET_PLANNER: &str = "tessera:packet/planner.v1";
    pub const PACKET_VERIFIER: &str = "tessera:packet/verifier.v1";
    pub const PACKET_RETRIEVER: &str = "tessera:packet/retriever.v1";
    pub const PACKET_SYNTHESIZER: &str = "tessera:packet/synthesizer.v1";
    pub const PACKET_VISUALIZER: &str = "tessera:packet/visualizer.v1";
    pub const PACKET_EXERCISE: &str = "tessera:packet/exercise.v1";

    pub const OUT_ROUTER: &str = "tessera:output/router.v1";
    pub const OUT_PLANNER: &str = "tessera:output/planner.v1";
    pub const OUT_RETRIEVER: &str = "tessera:output/retriever.v1";
    pub const OUT_SYNTHESIZER: &str = "tessera:output/synthesizer.v1";
    pub const OUT_VISUALIZER: &str = "tessera:output/visualizer.v1";
    pub const OUT_READER: &str = "tessera:output/reader.v1";
    pub const OUT_VERIFIER: &str = "tessera:output/verifier.v1";
    pub const OUT_EXERCISE: &str = "tessera:output/exercise.v1";
    pub const OUT_TUTOR: &str = "tessera:output/tutor.v1";

    /// Every id the build expects to exist, checked by a test so a renamed file
    /// cannot quietly leave a boundary unguarded.
    pub const ALL: &[&str] = &[
        EVENT_ENVELOPE,
        COMMON,
        VISUAL,
        STRUCTURED_SUMMARY,
        DOCTRINE_PACK,
        BUNDLE_MANIFEST,
        PACKET_ROUTER,
        PACKET_RETRIEVER,
        PACKET_PLANNER,
        PACKET_VERIFIER,
        PACKET_SYNTHESIZER,
        PACKET_VISUALIZER,
        PACKET_EXERCISE,
        OUT_ROUTER,
        OUT_PLANNER,
        OUT_RETRIEVER,
        OUT_SYNTHESIZER,
        OUT_VISUALIZER,
        OUT_READER,
        OUT_VERIFIER,
        OUT_EXERCISE,
        OUT_TUTOR,
    ];
}
