//! Pre and post tool hooks. Pattern 11.
//!
//! Doc 05 section 2: "Pre tool hooks enforce exclusions and rate limits before
//! any fetch; post tool hooks record provenance and content hashes after."
//! Doc 10 section 12: "Hooks deny excluded paths and domains before any fetch;
//! denials are logged."
//!
//! Doc 05 section 10 fixes the posture: "tolerant per assignment, strict on
//! hooks. A retriever may return nothing; it may never return something it was
//! told not to touch." A hook denial is a hard failure, never a caveat.
//!
//! One privacy rule shapes the API. When a hook denies, the reason the user sees
//! names the exclusion *category*, never the excluded item: telling someone
//! their answer omitted `Sensitive/board-pack-Q3.pdf` leaks the thing the
//! exclusion existed to protect. [`Denial::category`] is what surfaces;
//! [`Denial::target`] stays in the local log.

use serde::{Deserialize, Serialize};

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Pre,
    Post,
}

/// What a hook is being asked about.
#[derive(Debug, Clone)]
pub struct HookContext<'a> {
    pub retriever_id: &'a str,
    pub run_id: &'a str,
    /// The query about to be sent, for a pre hook on a search.
    pub query: Option<&'a str>,
    /// The path or url about to be opened.
    pub target: Option<&'a str>,
    /// Doctrine plus profile exclusions, already merged by the caller.
    pub excluded_paths: &'a [String],
    pub denied_domains: &'a [String],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Denial {
    pub hook_id: String,
    /// What the user is told. Names the class of exclusion, not the item.
    pub category: String,
    /// What the local log records. Never leaves the machine.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(Denial),
}

impl Decision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Decision::Allow)
    }
}

pub trait Hook: Send + Sync {
    fn id(&self) -> &str;
    fn phase(&self) -> Phase;
    fn check(&self, ctx: &HookContext<'_>) -> Decision;
}

/// Runs hooks in order and stops at the first denial.
#[derive(Default)]
pub struct HookSet {
    hooks: Vec<Box<dyn Hook>>,
}

impl HookSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pre hook set from doc 05 section 15. `rate_limit` lives in the
    /// provider layer, which is the only place that knows the per provider
    /// queue depth (doc 10 section 6).
    pub fn retriever_defaults() -> Self {
        Self::new()
            .with(ExcludePaths)
            .with(DenyDomains)
            .with(NoPiiInQuery::default())
    }

    pub fn with(mut self, hook: impl Hook + 'static) -> Self {
        self.hooks.push(Box::new(hook));
        self
    }

    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Returns the first denial, if any. A denial stops the assignment: doc 05
    /// section 10 makes `hook_denied` a hard stop, so there is no point asking
    /// the remaining hooks.
    pub fn run(&self, phase: Phase, ctx: &HookContext<'_>) -> Option<Denial> {
        for hook in self.hooks.iter().filter(|h| h.phase() == phase) {
            if let Decision::Deny(d) = hook.check(ctx) {
                return Some(d);
            }
        }
        None
    }
}

/// Deny a path under an excluded folder. Doc 02 section 5.3 plants facts in a
/// `Sensitive` subfolder that must never appear in an answer while the exclusion
/// is on, and doc 05 section 12 requires exclusion compliance of 1.00.
pub struct ExcludePaths;

impl Hook for ExcludePaths {
    fn id(&self) -> &str {
        "exclude_paths"
    }

    fn phase(&self) -> Phase {
        Phase::Pre
    }

    fn check(&self, ctx: &HookContext<'_>) -> Decision {
        let Some(target) = ctx.target else {
            return Decision::Allow;
        };
        // Compare on a normalised path so a backslash, a trailing separator or a
        // case difference cannot walk around the exclusion on Windows.
        let normalised = normalise_path(target);
        for pattern in ctx.excluded_paths {
            let p = normalise_path(pattern);
            if p.is_empty() {
                continue;
            }
            let matched = normalised == p
                || normalised.starts_with(&format!("{p}/"))
                || normalised.split('/').any(|segment| segment == p);
            if matched {
                return Decision::Deny(Denial {
                    hook_id: self.id().into(),
                    category: "an excluded folder".into(),
                    target: target.to_string(),
                });
            }
        }
        Decision::Allow
    }
}

fn normalise_path(p: &str) -> String {
    p.replace('\\', "/").trim_matches('/').to_lowercase()
}

/// Deny a fetch to a denied domain. Doc 05 section 8.1: the doctrine denylist
/// plus the user's own.
pub struct DenyDomains;

impl Hook for DenyDomains {
    fn id(&self) -> &str {
        "deny_domains"
    }

    fn phase(&self) -> Phase {
        Phase::Pre
    }

    fn check(&self, ctx: &HookContext<'_>) -> Decision {
        let Some(target) = ctx.target else {
            return Decision::Allow;
        };
        let Some(host) = host_of(target) else {
            return Decision::Allow;
        };
        for domain in ctx.denied_domains {
            let d = domain.trim_start_matches('.').to_lowercase();
            if d.is_empty() {
                continue;
            }
            // Match the host or any subdomain of it, never a suffix that merely
            // ends the same way: `notevil.com` is not a subdomain of `evil.com`.
            if host == d || host.ends_with(&format!(".{d}")) {
                return Decision::Deny(Denial {
                    hook_id: self.id().into(),
                    category: "a denied domain".into(),
                    target: target.to_string(),
                });
            }
        }
        Decision::Allow
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = authority.rsplit_once(':').map_or(authority, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            authority
        }
    });
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

/// Keep account and identity numbers out of a query that leaves the machine.
///
/// Doc 05 section 15 lists it as a retriever pre hook; doc 03 section 8.4 runs
/// the same class of check on the request itself, where it blocks the run and
/// asks the user to remove the data.
pub struct NoPiiInQuery {
    patterns: Vec<(&'static str, regex::Regex)>,
}

impl Default for NoPiiInQuery {
    fn default() -> Self {
        let compile = |name: &'static str, p: &str| regex::Regex::new(p).map(|r| (name, r)).ok();
        Self {
            patterns: [
                // IBAN: two letters, two check digits, then up to thirty.
                compile("an account number", r"(?i)\b[A-Z]{2}\d{2}[A-Z0-9]{10,30}\b"),
                // A long unbroken digit run is a card or account number often
                // enough that sending it is not worth the convenience.
                compile("an account number", r"\b\d{13,19}\b"),
                // Dutch BSN and similar nine digit national identifiers.
                compile("a national identifier", r"\b\d{9}\b"),
                // US social security.
                compile("a national identifier", r"\b\d{3}-\d{2}-\d{4}\b"),
            ]
            .into_iter()
            .flatten()
            .collect(),
        }
    }
}

impl Hook for NoPiiInQuery {
    fn id(&self) -> &str {
        "no_pii_in_query"
    }

    fn phase(&self) -> Phase {
        Phase::Pre
    }

    fn check(&self, ctx: &HookContext<'_>) -> Decision {
        let Some(query) = ctx.query else {
            return Decision::Allow;
        };
        for (category, pattern) in &self.patterns {
            if pattern.is_match(query) {
                return Decision::Deny(Denial {
                    hook_id: self.id().into(),
                    // The matched text is deliberately absent: it is the thing
                    // being protected.
                    category: (*category).to_string(),
                    target: "[redacted]".into(),
                });
            }
        }
        Decision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        target: Option<&'a str>,
        query: Option<&'a str>,
        excluded: &'a [String],
        denied: &'a [String],
    ) -> HookContext<'a> {
        HookContext {
            retriever_id: "local",
            run_id: "r1",
            query,
            target,
            excluded_paths: excluded,
            denied_domains: denied,
        }
    }

    #[test]
    fn an_excluded_folder_is_denied_however_it_is_spelled() {
        // Doc 05 section 12 requires exclusion compliance of 1.00, so a
        // separator or a case difference must not walk around it.
        let excluded = vec!["Sensitive".to_string()];
        let hooks = HookSet::retriever_defaults();
        for target in [
            "Sensitive/board-pack.pdf",
            "sensitive/board-pack.pdf",
            r"Policies\Sensitive\board-pack.pdf",
            "/Policies/Sensitive/2026/board-pack.pdf",
        ] {
            let d = hooks
                .run(Phase::Pre, &ctx(Some(target), None, &excluded, &[]))
                .unwrap_or_else(|| panic!("`{target}` must be denied"));
            assert_eq!(d.hook_id, "exclude_paths");
        }
    }

    #[test]
    fn a_folder_that_merely_starts_the_same_is_allowed() {
        let excluded = vec!["Sensitive".to_string()];
        let hooks = HookSet::retriever_defaults();
        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("SensitivityAnalysis/notes.md"), None, &excluded, &[])
                )
                .is_none()
        );
    }

    #[test]
    fn a_denial_names_the_category_and_not_the_item() {
        // Doc 05 section 10: the caveat names the exclusion category without
        // naming the excluded item.
        let excluded = vec!["Sensitive".to_string()];
        let hooks = HookSet::retriever_defaults();
        let d = hooks
            .run(
                Phase::Pre,
                &ctx(Some("Sensitive/merger.docx"), None, &excluded, &[]),
            )
            .expect("denied");
        assert_eq!(d.category, "an excluded folder");
        assert!(
            !d.category.contains("merger"),
            "the category must not leak the filename"
        );
    }

    #[test]
    fn a_denied_domain_covers_its_subdomains_but_not_a_lookalike() {
        let denied = vec!["contentfarm.example".to_string()];
        let hooks = HookSet::retriever_defaults();

        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("https://contentfarm.example/a"), None, &[], &denied)
                )
                .is_some()
        );
        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("https://www.contentfarm.example/a"), None, &[], &denied)
                )
                .is_some()
        );
        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("https://notcontentfarm.example/a"), None, &[], &denied)
                )
                .is_none(),
            "a suffix match is not a subdomain match"
        );
    }

    #[test]
    fn a_port_or_userinfo_does_not_hide_a_denied_host() {
        let denied = vec!["evil.example".to_string()];
        let hooks = HookSet::retriever_defaults();
        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("https://evil.example:8443/x"), None, &[], &denied)
                )
                .is_some()
        );
        assert!(
            hooks
                .run(
                    Phase::Pre,
                    &ctx(Some("https://user@evil.example/x"), None, &[], &denied)
                )
                .is_some()
        );
    }

    #[test]
    fn an_account_number_never_reaches_a_search_provider() {
        let hooks = HookSet::retriever_defaults();
        let d = hooks
            .run(
                Phase::Pre,
                &ctx(None, Some("balance for NL91ABNA0417164300"), &[], &[]),
            )
            .expect("denied");
        assert_eq!(d.hook_id, "no_pii_in_query");
        assert_eq!(
            d.target, "[redacted]",
            "the hook must not echo what it is protecting"
        );
    }

    #[test]
    fn a_national_identifier_is_caught() {
        let hooks = HookSet::retriever_defaults();
        assert!(
            hooks
                .run(Phase::Pre, &ctx(None, Some("case for 123456782"), &[], &[]))
                .is_some()
        );
        assert!(
            hooks
                .run(Phase::Pre, &ctx(None, Some("ssn 123-45-6789"), &[], &[]))
                .is_some()
        );
    }

    #[test]
    fn an_ordinary_query_passes_every_hook() {
        let hooks = HookSet::retriever_defaults();
        let excluded = vec!["Sensitive".to_string()];
        let denied = vec!["contentfarm.example".to_string()];
        let c = ctx(
            Some("https://regulator.example/car3/article-92"),
            Some("CAR3 article 92 trading book treatment"),
            &excluded,
            &denied,
        );
        assert!(hooks.run(Phase::Pre, &c).is_none());
    }

    #[test]
    fn an_article_number_is_not_mistaken_for_an_identifier() {
        // A four digit year and a two or three digit article number are the
        // everyday shape of a regulatory query. Denying those would make the
        // finance pack unusable.
        let hooks = HookSet::retriever_defaults();
        for query in [
            "CAR3 article 92 as amended in 2026",
            "PSD-S recital 41",
            "OG-2025 paragraph 7",
        ] {
            assert!(
                hooks.run(Phase::Pre, &ctx(None, Some(query), &[], &[])).is_none(),
                "`{query}` must pass"
            );
        }
    }
}
