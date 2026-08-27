//! Doctrine packs. Doc 01 section 2 and section 4.17.
//!
//! Doc 12 operating principle 4: "Doctrine is data. Packs are JSON files with a
//! schema; no domain rule in code." Doc 10 principle 6 says the same: packs are
//! files, versioned, importable, editable in the Profile, and no pack content
//! lives in code.
//!
//! This crate loads and validates them. It contains no rule, only the shapes a
//! rule is written in, which is what lets a second vertical ship as a file.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tessera_schema::{Registry, ids};

/// The packs that ship in the app bundle. Doc 10 section 9.
pub static BUILT_IN: &[(&str, &str)] = &[
    ("general", include_str!("../../../packs/general.json")),
    // Doc 11 mission: "Finance is the first doctrine pack." The rules are the
    // ones the synthetic twin below is scored on, so what differs between them
    // is the source hierarchy and the vocabulary, and nothing that decides
    // whether a card passes.
    ("finance-eu", include_str!("../../../packs/finance-eu.json")),
    // Doc 02 section 4: the sibling of finance-eu with the synthetic issuers
    // substituted in, so evaluation output can be quoted without a real
    // regulator appearing in it. Doc 02 section 10.1 loads it for every run.
    (
        "finance-eu-synthetic",
        include_str!("../../../packs/finance-eu-synthetic.json"),
    ),
];

#[derive(Debug, thiserror::Error)]
pub enum DoctrineError {
    #[error("pack `{code}` is malformed: {detail}")]
    Malformed { code: String, detail: String },

    #[error("no pack with code `{0}`")]
    Unknown(String),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DoctrineError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Audience {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vocabulary_notes: Option<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRank {
    pub class: String,
    #[serde(default)]
    pub issuer_pattern: Option<String>,
    pub trust_rank: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FreshnessClass {
    pub max_age_days: i64,
    /// `flag` or `rerun`.
    pub on_stale: String,
}

/// One flag rule. Doc 07 section B8 runs these; the pack decides which exist,
/// at what severity, and in which modes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlagRule {
    pub rule_id: String,
    pub severity: String,
    pub description: String,
    /// `deterministic:<name>` or `model:<prompt_id>`.
    pub detector: String,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Absent means every mode.
    #[serde(default)]
    pub modes: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl FlagRule {
    pub fn is_deterministic(&self) -> bool {
        self.detector.starts_with("deterministic:")
    }

    /// The detector name without its kind prefix.
    pub fn detector_name(&self) -> &str {
        self.detector
            .split_once(':')
            .map_or(self.detector.as_str(), |(_, n)| n)
    }

    pub fn runs_in(&self, mode: &str) -> bool {
        self.enabled && (self.modes.is_empty() || self.modes.iter().any(|m| m == mode))
    }
}

/// Doc 14 section 3.2's learning doctrine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LearningTemplates {
    /// Doc 14 section 3.4: the plan is ordered foundation to detail.
    #[serde(default = "default_shapes")]
    pub curriculum_shapes: Vec<String>,
    /// Doc 14 section 3.6: how many correct checks make a concept mastered.
    #[serde(default = "default_mastery")]
    pub mastery_threshold: u32,
    /// Doc 14 section 3.2: intake question templates per domain.
    #[serde(default)]
    pub intake_questions: Vec<String>,
}

fn default_shapes() -> Vec<String> {
    // Doc 14 section 3.8's fallback, which is also the sensible default: what it
    // is, how it works, who is involved.
    vec!["foundation".into(), "mechanism".into(), "landscape".into()]
}

fn default_mastery() -> u32 {
    2
}

impl Default for LearningTemplates {
    fn default() -> Self {
        Self {
            curriculum_shapes: default_shapes(),
            mastery_threshold: default_mastery(),
            intake_questions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RetrieverConfig {
    pub id: String,
    #[serde(default)]
    pub enabled_by_default: bool,
    #[serde(default)]
    pub config: BTreeMap<String, Value>,
    /// The Planner may add to this, never remove from it. Doc 04 section 5.
    #[serde(default)]
    pub must_exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VisualPreferences {
    #[serde(default)]
    pub type_preferences: BTreeMap<String, String>,
    #[serde(default = "default_max_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_max_rows")]
    pub max_rows: usize,
    /// Doc 06 section B14 open question 2: on for the general pack, off for
    /// finance, both overridable in Profile.
    #[serde(default)]
    pub generated_images: bool,
}

fn default_max_nodes() -> usize {
    18
}

fn default_max_rows() -> usize {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct WritingRules {
    #[serde(default)]
    pub units: Option<String>,
    #[serde(default)]
    pub spelling: Option<String>,
    #[serde(default)]
    pub sentence_max_words: Option<usize>,
    /// House style forbids them. Doc 11 section 9.
    #[serde(default)]
    pub dashes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExerciseTemplate {
    pub id: String,
    pub item_kinds: Vec<String>,
    #[serde(default)]
    pub items_per_card_max: Option<usize>,
    #[serde(default)]
    pub options: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SensitivityRule {
    pub rule_id: String,
    pub detector: String,
    pub severity: String,
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// A loaded pack. Doc 01 section 4.17.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctrinePack {
    pub code: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub domain_vocabulary: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub depth_hints: BTreeMap<String, String>,
    /// Doc 03 open question 3: the pack may set a minimum the user cannot lower.
    #[serde(default)]
    pub minimum_depth: BTreeMap<String, String>,

    pub audiences: Vec<Audience>,
    pub source_hierarchy: Vec<SourceRank>,
    pub freshness_classes: BTreeMap<String, FreshnessClass>,
    pub flag_rules: Vec<FlagRule>,
    #[serde(default)]
    pub sensitivity_rules: Vec<SensitivityRule>,
    pub retrievers: Vec<RetrieverConfig>,
    #[serde(default)]
    pub visual_preferences: VisualPreferences,
    #[serde(default)]
    pub writing_rules: WritingRules,
    pub exercise_templates: Vec<ExerciseTemplate>,
    /// Doc 14 section 3.2: the shapes a curriculum takes and how many correct
    /// checks count as mastery are doctrine, not substrate.
    #[serde(default)]
    pub learning_templates: LearningTemplates,
    /// Doc 07 section A2: what the Reader looks for first is doctrine.
    ///
    /// The finance pack names figures, dates and article references. A pack that
    /// names none leaves this empty, and the Reader asks for nothing in
    /// particular rather than being handed a guess as though it were doctrine.
    #[serde(default)]
    pub reader_extract_first: Vec<String>,
    #[serde(default)]
    pub rulings: Vec<Value>,
}

impl DoctrinePack {
    /// Parse and validate. The schema check runs first, so a malformed pack is
    /// rejected with a JSON pointer rather than a serde message about a field
    /// nobody outside this crate has heard of.
    pub fn parse(registry: &Registry, raw: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(raw)?;
        let code = value
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        registry
            .validate(ids::DOCTRINE_PACK, &value)
            .map_err(|e| DoctrineError::Malformed {
                code: code.clone(),
                detail: e.to_string(),
            })?;

        serde_json::from_value(value).map_err(|e| DoctrineError::Malformed {
            code,
            detail: e.to_string(),
        })
    }

    /// Doc 03 section 8.2 step 3, and open question 3 resolved as proposed: a
    /// pack may set a minimum depth the user cannot lower.
    pub fn floor_depth(&self, domain: &str, chosen: &str) -> &str {
        let rank = |d: &str| match d {
            "fast" => 0,
            "deep" => 1,
            _ => 2,
        };
        match self.minimum_depth.get(domain) {
            Some(min) if rank(min) > rank(chosen) => match min.as_str() {
                "fast" => "fast",
                "deep" => "deep",
                _ => "research",
            },
            _ => match chosen {
                "fast" => "fast",
                "deep" => "deep",
                _ => "research",
            },
        }
    }

    /// The trust rank doctrine assigns a source. Doc 05 section 5: never the
    /// retriever's own judgment. An issuer pattern beats a bare class match.
    pub fn trust_rank(&self, class: &str, issuer: Option<&str>) -> i64 {
        let mut best: Option<i64> = None;
        for rank in &self.source_hierarchy {
            if rank.class != class {
                continue;
            }
            match (&rank.issuer_pattern, issuer) {
                (Some(pattern), Some(issuer)) if issuer.contains(pattern.as_str()) => {
                    return rank.trust_rank;
                }
                (None, _) => best = Some(best.map_or(rank.trust_rank, |b: i64| b.min(rank.trust_rank))),
                _ => {}
            }
        }
        // An unranked class is the least trusted thing on the board, not the most.
        best.unwrap_or_else(|| {
            self.source_hierarchy
                .iter()
                .map(|r| r.trust_rank)
                .max()
                .unwrap_or(9)
                + 1
        })
    }

    /// The deterministic rules that apply in this mode, in pack order.
    pub fn deterministic_rules(&self, mode: &str) -> impl Iterator<Item = &FlagRule> {
        self.flag_rules
            .iter()
            .filter(move |r| r.is_deterministic() && r.runs_in(mode))
    }

    pub fn rule(&self, rule_id: &str) -> Option<&FlagRule> {
        self.flag_rules.iter().find(|r| r.rule_id == rule_id)
    }

    /// Every folder the pack says a retriever must never open. Merged with the
    /// profile's own exclusions before the hooks run.
    pub fn must_exclude(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .retrievers
            .iter()
            .flat_map(|r| r.must_exclude.iter().cloned())
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// A pack file that did not load, and why.
///
/// A built in pack that fails to parse is a build error and stops the app. An
/// imported one cannot be, because the file belongs to the person and a typo in
/// it must not lock them out of their own boards. So it is skipped, and the
/// reason is carried here to the Profile page rather than to a log nobody
/// reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackProblem {
    pub file: String,
    pub detail: String,
}

/// The packs available to a profile: the built in ones plus any imported.
pub struct PackLibrary {
    packs: BTreeMap<String, DoctrinePack>,
    /// Codes that came from a file in the profile folder rather than the app
    /// bundle. Doc 10 section 9: a built in pack is the same on every machine
    /// and an imported one is not, which is a difference the Doctrine page
    /// shows.
    imported: BTreeSet<String>,
    problems: Vec<PackProblem>,
}

impl PackLibrary {
    /// Load every built in pack. A pack that does not validate is a build error
    /// wearing a runtime disguise, so this fails rather than skipping it.
    pub fn load_built_in(registry: &Registry) -> Result<Self> {
        let mut packs = BTreeMap::new();
        for (code, raw) in BUILT_IN {
            let pack = DoctrinePack::parse(registry, raw)?;
            if &pack.code != code {
                return Err(DoctrineError::Malformed {
                    code: (*code).to_string(),
                    detail: format!("the file declares code `{}`", pack.code),
                });
            }
            packs.insert(pack.code.clone(), pack);
        }
        Ok(Self {
            packs,
            imported: BTreeSet::new(),
            problems: Vec::new(),
        })
    }

    /// Load every `*.json` in `dir` as an imported pack.
    ///
    /// Sorted, so two machines with the same folder end up with the same
    /// library. A file that does not validate is recorded and skipped: see
    /// [`PackProblem`]. A missing folder is not a problem, because most
    /// profiles have imported nothing.
    pub fn load_imported(&mut self, registry: &Registry, dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();

        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(e) => {
                    self.problems.push(PackProblem {
                        file: name,
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            match DoctrinePack::parse(registry, &raw) {
                Ok(pack) => {
                    // A built in code is not a name an imported file may take.
                    // Boards pin the pack they were answered under by code and
                    // version, so a file that renamed `general` would change
                    // what a hundred existing boards claim to have been judged
                    // by.
                    if BUILT_IN.iter().any(|(code, _)| *code == pack.code) {
                        self.problems.push(PackProblem {
                            file: name,
                            detail: format!("`{}` is the code of a pack that ships with the app", pack.code),
                        });
                        continue;
                    }
                    self.imported.insert(pack.code.clone());
                    self.packs.insert(pack.code.clone(), pack);
                }
                Err(e) => self.problems.push(PackProblem {
                    file: name,
                    detail: e.to_string(),
                }),
            }
        }
    }

    pub fn add(&mut self, pack: DoctrinePack) {
        self.imported.insert(pack.code.clone());
        self.packs.insert(pack.code.clone(), pack);
    }

    /// Whether this code ships with the app.
    pub fn is_built_in(code: &str) -> bool {
        BUILT_IN.iter().any(|(c, _)| *c == code)
    }

    pub fn is_imported(&self, code: &str) -> bool {
        self.imported.contains(code)
    }

    /// The pack files that did not load, in the order they were read.
    pub fn problems(&self) -> &[PackProblem] {
        &self.problems
    }

    pub fn get(&self, code: &str) -> Result<&DoctrinePack> {
        self.packs
            .get(code)
            .ok_or_else(|| DoctrineError::Unknown(code.to_string()))
    }

    pub fn codes(&self) -> impl Iterator<Item = &str> {
        self.packs.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> PackLibrary {
        let registry = Registry::load().expect("registry");
        PackLibrary::load_built_in(&registry).expect("every shipped pack must validate")
    }

    fn write_pack(dir: &std::path::Path, file: &str, code: &str) {
        let mut pack: Value = serde_json::from_str(BUILT_IN[0].1).expect("the shipped pack parses");
        pack["code"] = Value::String(code.to_string());
        std::fs::create_dir_all(dir).expect("dir");
        std::fs::write(dir.join(file), pack.to_string()).expect("file");
    }

    #[test]
    fn an_imported_pack_joins_the_library_and_says_it_was_imported() {
        let registry = Registry::load().expect("registry");
        let dir = std::env::temp_dir().join(format!("tessera-packs-{}", ulid_ish()));
        write_pack(&dir, "house.json", "house-rules");

        let mut lib = library();
        lib.load_imported(&registry, &dir);

        assert!(lib.get("house-rules").is_ok());
        assert!(lib.is_imported("house-rules"));
        assert!(!lib.is_imported("general"));
        assert!(!PackLibrary::is_built_in("house-rules"));
        assert!(lib.problems().is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_may_not_take_a_shipped_code_and_the_shipped_pack_stands() {
        // Dropping a file into the folder by hand reaches this path rather than
        // the import RPC, and it must not be the way round the same rule.
        let registry = Registry::load().expect("registry");
        let dir = std::env::temp_dir().join(format!("tessera-packs-{}", ulid_ish()));
        write_pack(&dir, "mine.json", "general");

        let mut lib = library();
        lib.load_imported(&registry, &dir);

        assert!(!lib.is_imported("general"));
        assert_eq!(lib.get("general").expect("general").version, "1.0.0");
        assert_eq!(lib.problems().len(), 1);
        assert!(lib.problems()[0].detail.contains("ships with the app"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_a_pack_is_a_problem_rather_than_a_panic() {
        let registry = Registry::load().expect("registry");
        let dir = std::env::temp_dir().join(format!("tessera-packs-{}", ulid_ish()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("notes.json"), "{ half a file").expect("file");
        std::fs::write(dir.join("readme.txt"), "not json, not read").expect("file");

        let mut lib = library();
        lib.load_imported(&registry, &dir);

        assert_eq!(lib.problems().len(), 1, "only the json file was read");
        assert_eq!(lib.problems()[0].file, "notes.json");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_that_is_not_there_is_not_a_problem() {
        // Most profiles have imported nothing.
        let registry = Registry::load().expect("registry");
        let mut lib = library();
        lib.load_imported(&registry, std::path::Path::new("/no/such/folder/anywhere"));
        assert!(lib.problems().is_empty());
    }

    /// A unique enough suffix without pulling a dependency in for it.
    fn ulid_ish() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    #[test]
    fn the_general_pack_ships_and_validates() {
        let lib = library();
        let pack = lib.get("general").expect("general");
        assert_eq!(pack.version, "1.0.0");
        assert!(!pack.flag_rules.is_empty());
    }

    #[test]
    fn three_packs_ship() {
        // Doc 12 phase 10 names three, and doc 11's mission makes finance the
        // first doctrine pack rather than an optional one.
        let lib = library();
        let mut codes: Vec<&str> = lib.codes().collect();
        codes.sort_unstable();
        assert_eq!(codes, ["finance-eu", "finance-eu-synthetic", "general"]);
    }

    #[test]
    fn the_synthetic_twin_carries_the_same_rules_as_the_pack_it_stands_for() {
        // Doc 02 section 4: the twin exists "so a score on the corpus is
        // comparable with the shipped pack". That only holds while the two
        // agree on every rule and severity. What may differ is the source
        // hierarchy and the vocabulary, which name issuers rather than decide
        // whether a card passes.
        let lib = library();
        let real = lib.get("finance-eu").expect("finance-eu");
        let twin = lib.get("finance-eu-synthetic").expect("finance-eu-synthetic");

        let rules = |p: &DoctrinePack| {
            let mut out: Vec<(String, String, String)> = p
                .flag_rules
                .iter()
                .map(|r| (r.rule_id.clone(), r.severity.clone(), r.detector.clone()))
                .collect();
            out.sort();
            out
        };
        assert_eq!(
            rules(real),
            rules(twin),
            "a score on the corpus no longer transfers to the shipped pack"
        );

        // The hierarchy may name different issuers, and the twin does: it ranks
        // one synthetic web publisher above the rest, which pushes the generic
        // web rank down by one and `user_supplied` with it. What may not differ
        // is the order the classes come in, because that is what decides which
        // source wins a contradiction, and a score on the corpus has to
        // transfer.
        let order = |p: &DoctrinePack| {
            let mut out: Vec<(i64, String)> = p
                .source_hierarchy
                .iter()
                .filter(|r| r.issuer_pattern.is_none())
                .map(|r| (r.trust_rank, r.class.clone()))
                .collect();
            out.sort();
            out.into_iter().map(|(_, class)| class).collect::<Vec<_>>()
        };
        assert_eq!(
            order(real),
            order(twin),
            "the twin ranks the source classes in a different order from the pack it stands for"
        );

        // Doc 16 section 3.3: a page ranks 4 in the finance pack, below
        // external sources and above own_card.
        for pack in [real, twin] {
            let page = pack
                .source_hierarchy
                .iter()
                .find(|r| r.class == "page")
                .expect("the finance pack ranks a page");
            assert_eq!(page.trust_rank, 4);
        }

        // And every pack that ships can rank a page at all. An unranked class
        // is the least trusted, which would put a person's own notes below a
        // blog.
        for code in ["general", "finance-eu", "finance-eu-synthetic"] {
            let pack = lib.get(code).expect(code);
            assert!(
                pack.source_hierarchy.iter().any(|r| r.class == "page"),
                "{code} leaves a page unranked, which ranks it last"
            );
        }
    }

    #[test]
    fn the_memory_rule_is_doctrine_rather_than_code() {
        // Doc 12 principle 4: packs are JSON and no domain rule lives in code.
        // Doc 05 v0.2 line 106 names this one, and until M12 it existed only in
        // a test fixture and a comment: the Verifier had no such rule at all.
        let lib = library();
        for code in ["general", "finance-eu", "finance-eu-synthetic"] {
            let pack = lib.get(code).expect(code);
            let rule = pack
                .flag_rules
                .iter()
                .find(|r| r.rule_id == "own_card_sole_support")
                .unwrap_or_else(|| panic!("{code} does not carry the memory rule"));
            assert_eq!(rule.severity, "block", "{code}");
            assert_eq!(rule.detector, "deterministic:own_card_sole_support", "{code}");
        }
    }

    #[test]
    fn every_shipped_rule_names_a_detector_kind() {
        // Doc 07 section B10: a rule with a missing detector is skipped and the
        // Profile is told the pack is malformed. Better to not ship one.
        let lib = library();
        for code in lib.codes().map(str::to_string).collect::<Vec<_>>() {
            let pack = lib.get(&code).expect("pack");
            for rule in &pack.flag_rules {
                assert!(
                    rule.detector.starts_with("deterministic:") || rule.detector.starts_with("model:"),
                    "{code}/{} has detector `{}`",
                    rule.rule_id,
                    rule.detector
                );
            }
        }
    }

    #[test]
    fn fast_mode_runs_only_the_rules_that_need_no_passages() {
        // Doc 07 section B5: in fast mode every verdict is unchecked and the only
        // rules that run are the deterministic ones that do not need passages.
        let lib = library();
        let pack = lib.get("general").expect("general");
        let fast: Vec<&str> = pack
            .deterministic_rules("fast")
            .map(|r| r.rule_id.as_str())
            .collect();

        assert!(fast.contains(&"fast_mode_notice"));
        assert!(
            !fast.contains(&"numeric_without_citation"),
            "a rule that reads citations cannot run where there are none"
        );
        assert!(!fast.contains(&"computed_value"));

        let deep: Vec<&str> = pack
            .deterministic_rules("deep")
            .map(|r| r.rule_id.as_str())
            .collect();
        assert!(deep.contains(&"numeric_without_citation"));
        assert!(!deep.contains(&"fast_mode_notice"));
    }

    #[test]
    fn a_disabled_rule_runs_in_no_mode() {
        // Doc 02 section 10.3: a rule whose false positive rate exceeds 0.10 is
        // disabled by default in the pack and listed as an open item.
        let mut rule = FlagRule {
            rule_id: "noisy".into(),
            severity: "warn".into(),
            description: "".into(),
            detector: "deterministic:noisy".into(),
            params: BTreeMap::new(),
            enabled: true,
            modes: vec![],
        };
        assert!(rule.runs_in("deep"));
        rule.enabled = false;
        assert!(!rule.runs_in("deep"));
        assert!(!rule.runs_in("fast"));
    }

    #[test]
    fn trust_rank_prefers_an_issuer_match_over_a_class_match() {
        let pack = DoctrinePack {
            source_hierarchy: vec![
                SourceRank {
                    class: "web".into(),
                    issuer_pattern: None,
                    trust_rank: 4,
                },
                SourceRank {
                    class: "web".into(),
                    issuer_pattern: Some("regulator.example".into()),
                    trust_rank: 1,
                },
            ],
            ..blank()
        };
        assert_eq!(pack.trust_rank("web", Some("news.regulator.example")), 1);
        assert_eq!(pack.trust_rank("web", Some("someblog.example")), 4);
    }

    #[test]
    fn an_unranked_class_is_the_least_trusted_not_the_most() {
        // Defaulting to rank 0 would make an unknown source outrank a regulator.
        let lib = library();
        let pack = lib.get("general").expect("general");
        let known = pack.trust_rank("regulatory", None);
        let unknown = pack.trust_rank("something_new", None);
        assert!(
            unknown > known,
            "unknown {unknown} must rank worse than regulatory {known}"
        );
    }

    #[test]
    fn a_pack_minimum_depth_raises_but_never_lowers() {
        // Doc 03 open question 3, resolved as proposed.
        let pack = DoctrinePack {
            minimum_depth: BTreeMap::from([("capital".to_string(), "deep".to_string())]),
            ..blank()
        };
        assert_eq!(pack.floor_depth("capital", "fast"), "deep");
        assert_eq!(pack.floor_depth("capital", "research"), "research");
        assert_eq!(pack.floor_depth("payments", "fast"), "fast");
    }

    #[test]
    fn a_malformed_pack_is_refused_with_a_pointer() {
        let registry = Registry::load().expect("registry");
        let err = DoctrinePack::parse(&registry, r#"{"code":"x","version":"1"}"#)
            .expect_err("a version that is not semver must be refused");
        assert!(matches!(err, DoctrineError::Malformed { .. }));
    }

    #[test]
    fn the_general_pack_excludes_a_sensitive_folder_by_default() {
        // Doc 02 section 5.3 plants facts there; doc 05 section 12 requires
        // exclusion compliance of 1.00.
        let lib = library();
        assert!(
            lib.get("general")
                .expect("general")
                .must_exclude()
                .contains(&"Sensitive".to_string())
        );
    }

    fn blank() -> DoctrinePack {
        DoctrinePack {
            code: "test".into(),
            version: "1.0.0".into(),
            name: None,
            description: None,
            domains: vec![],
            domain_vocabulary: BTreeMap::new(),
            depth_hints: BTreeMap::new(),
            minimum_depth: BTreeMap::new(),
            audiences: vec![],
            source_hierarchy: vec![],
            freshness_classes: BTreeMap::new(),
            flag_rules: vec![],
            sensitivity_rules: vec![],
            retrievers: vec![],
            visual_preferences: VisualPreferences::default(),
            writing_rules: WritingRules::default(),
            exercise_templates: vec![],
            reader_extract_first: vec![],
            learning_templates: LearningTemplates::default(),
            rulings: vec![],
        }
    }
}
