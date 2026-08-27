# 17. Learning System v0.1

Register: working. Depends on: 01 (Concept), 05 (retrievers), 07 (Verifier), 08 (Exercise), 14 (Tutor), 15 (memory), 16 (Pages). Supersedes the session-only view of Learn mode in 14 with a persistent learning layer. The Tutor remains the agent that runs a lesson; a new **Learning Planner** owns the map.

## 1. Agreed understanding

Decisions from conversation, recorded here:

| Question | Decision | Consequence |
|---|---|---|
| Unit of the knowledge map | The shared Concept | Concepts gain learning state and prerequisite edges. Boards and lessons write to the same node. |
| What the tutor may teach from | Any domain; sources gathered during learning through the research retriever; the user chooses what to keep | Lessons are verified cards. Quality of sources is doctrine. Unverified explanation is allowed and labelled while sources are fetched. |
| Whose learning | Personal maps; doctrine packs may ship learning paths | A path seeds a map. No team view in v1. |
| Finding gaps | Self rating per concept, verified by checks as learning proceeds | Placement is cheap and honest; ratings are claims, checks are evidence. |
| Where it lives | A Map view for the whole; a board per lesson for the detail | Map is a board of `mode: map` rendered from concepts; lessons are boards of `mode: learn` linked from map nodes. |
| Spaced review over time | Deferred | Decay is modelled in the state machine now so scheduling can be added without a migration. |

The loop the product is built around: **learn** (map and lessons), **explore** (boards), **keep** (vault). Each feeds the others through the Concept graph.

## 2. The knowledge map

### 2.1 Concept amendments (01 v0.3)

| Field | Type | Notes |
|---|---|---|
| learning_state | enum unseen, exposed, rated, checked, mastered, decayed | Section 2.3. |
| self_rating | int 0 to 3 or null | 0 never heard of it, 1 heard of it, 2 can explain it, 3 can apply it. A claim, never evidence. |
| mastery | float 0 to 1 | Evidence based, from checks and board activity. Section 2.4. |
| difficulty_level | int 1 to 4 | The level of the last check the learner passed. Section 4. |
| last_evidence_at | timestamp | Drives decay later. |
| path_ids | ulid[] | Learning paths this concept belongs to. |

New entity **ConceptEdge** (prerequisite structure, distinct from ConceptLink which binds concepts to content): `from_concept_id, to_concept_id, relation: prerequisite_of | part_of | contrasts_with, proposed_by, status: proposed | confirmed, weight`. Prerequisites are proposed by the Learning Planner and confirmed by the learner or by a shipped path.

New entity **Mission**: `id, profile_id, statement (why the learner wants this), target_concept_ids, audience_id, created_at, status: active | paused | done`. One or more per profile. Every lesson is planned against an active mission so difficulty and examples fit the reason.

New entity **LearningPath** (in a doctrine pack or authored): `id, code, title, mission_template, concepts: [{concept_term, prerequisite_terms[], level_hint}], sources_hint: [{title, locator, class}]`. Loading a path creates or links the concepts, creates the edges as confirmed, and offers a mission.

### 2.2 What writes to the map

| Event | Effect on the concept |
|---|---|
| A card that links the concept is read (opened, scrolled, or hovered for 3 s) | unseen to exposed; `card.viewed.v1` is a new event |
| The learner rates the concept | rated; self_rating set |
| A check on the concept is answered | mastery updated; state to checked or mastered |
| A check on a dependent concept is passed | small positive evidence on the prerequisite |
| The learner saves a page that defines the concept | positive evidence, weight 0.1 |
| Time passes beyond a doctrine threshold with no evidence | mastered to decayed (computed, no scheduler needed) |

Exploration on ordinary boards counts. Someone who wanders through six verified cards on liquidity risk has six exposures on the map without ever opening a lesson.

### 2.3 Learning state machine

```
unseen ──► exposed ──► rated ──► checked ──► mastered ──► decayed
   │          │          │           │            │           │
   └──────────┴──────────┴───────────┴── rating or check at any time moves right or left
```

Transitions to the right need evidence except `rated`, which needs a rating. `checked` means at least one passed check at level 1 or 2. `mastered` means mastery at or above the doctrine threshold (default 0.8) with a passed check at level 3 or higher. `decayed` is `mastered` past the doctrine's freshness window without new evidence; it renders differently and the tutor prefers it for the next check. A failed check can move `mastered` back to `checked`.

### 2.4 Mastery

A single score per concept, updated on evidence with a simple weighted model that a later spaced-repetition scheduler can replace:

`mastery' = mastery + k * (outcome - mastery)`, with `outcome` 1 for a pass and 0 for a fail, and `k` scaled by the check level (0.15 at level 1, 0.35 at level 4) and reduced for a pass on a repeated item. Exposure adds 0.02 up to 0.2. Self rating sets a starting prior: 0, 0.15, 0.35, 0.5 for ratings 0 to 3, applied only when mastery is null. The rule that keeps it honest: a rating can never move mastery above 0.5; only checks can.

## 3. Placement by self rating

When a mission is created or a path is loaded, the Map view shows the concepts as tiles in prerequisite order and asks for a rating per concept with four tappable levels. The learner may skip any tile. Ratings are recorded as claims. The Learning Planner then picks the **frontier**: the lowest prerequisite level where rated concepts have a rating of 2 or more and mastery is still unverified. The first lesson checks the frontier before teaching anything, so an overconfident rating is caught within the first two questions and an underconfident one lets the learner skip ahead.

## 4. Adaptive checks

Four levels per concept, each a kind of Exercise item:

| Level | Kind | What it asks |
|---|---|---|
| 1 | recall | State the definition or the fact |
| 2 | explain | Choose the correct explanation of why or how |
| 3 | apply | Given a short scenario, pick the rule or outcome that applies |
| 4 | discriminate | Two near cases; which differs and why (contrast with a neighbouring concept) |

Adaptation rule, deterministic: pass at level n moves the next check on that concept to n+1; fail moves it to n-1 and opens a remedial card at that level; two fails at level 1 open a card on the concept's strongest prerequisite instead. Items are generated by the Exercise agent from verified cards on the lesson board first, then from verified cards anywhere on the map, then, when none exist, the tutor requests a card for that concept before checking. No item is ever generated from unverified text.

Difficulty across concepts follows the frontier: the tutor works the lowest unverified prerequisite before its dependents, so the learner meets ideas in an order that keeps working memory small.

## 5. Lessons

A lesson is a board of `mode: learn`, linked from a map node, planned by the Learning Planner and run by the Tutor as in 14, with these additions:

- The plan targets one or two concepts at the frontier plus their immediate prerequisites, and names the mission.
- Cards are requested at deep depth with the **research retriever** enabled: web search steered by the doctrine's quality ranking for the domain (general pack: standards bodies, textbooks, primary papers, official documentation, then reputable explainers), plus the vault and boards retrievers. The learner's sources hint from a path is passed to the Planner as `must_include` locators.
- While cards are in flight the tutor may explain in the panel from model knowledge, labelled "unverified, sources loading"; the explanation is never cited and never used to generate checks.
- Each lesson ends with a **learning record**: a page generated from the lesson's verified cards and check outcomes (what was covered, what was checked, what remains), saved to the vault under `vault/learning/<mission>/<date>.md` with citations carried. The learner is asked which cards to keep as pages besides the record.

## 6. The Map view

A board of `mode: map` rendered from concepts rather than stored cards. Nodes are concepts sized by number of linked cards and coloured by learning state (unseen grey outline, exposed slate, rated amber outline, checked olive, mastered olive filled, decayed olive dashed). Edges are confirmed prerequisites; proposed ones are dotted. The frontier is a subtle band. Clicking a node opens a side panel: definition, audience definitions, rating, mastery, last evidence, linked cards and pages, "Start lesson", "Check me now", "Explore on a board". Filters by mission, by path, by state. The map is layout-managed (layered by prerequisite depth) and not hand-arranged, to keep it a view rather than a second canvas.

Home shows, per mission, the fraction of concepts at checked or better and the current frontier concept.

## 7. Agents

**Learning Planner** (new, eleventh agent). Reads the map, missions, paths, and recent evidence. Writes: prerequisite proposals for new concepts (one model call with the medium alias, constrained to the concepts on the map plus at most three new ones), the frontier, the next lesson plan (concept targets, prerequisites to include, level to open at). Runs on mission creation, path load, lesson end, and on demand ("What next?"). Deterministic where possible: frontier selection and level selection are rules; only decomposition of a new topic into concepts and prerequisites is model work, and it is proposed, not applied.

**Tutor** (14) runs the lesson. Its check selection now comes from the Planner's level and concept targets rather than from a free choice.

**Exercise** (08) gains the four kinds and level metadata on items; the distractor check gains "not a true statement about a neighbouring concept" for level 4.

**Retrievers** (05) gain a `research` profile of the web retriever: quality ranking from doctrine, more fetches per assignment, and a preference for primary sources; used by lessons and available to Research depth generally.

## 8. Doctrine additions

Per pack: quality ranking of source classes and issuer patterns for learning; mastery threshold; decay window per domain; check templates per level; optional learning paths; whether unverified panel explanations are allowed (finance: yes, but never for numbers or obligations).

## 9. Events

`mission.created.v1`, `mission.updated.v1`, `path.loaded.v1`, `concept.rated.v1`, `concept.edge_proposed.v1`, `concept.edge_confirmed.v1`, `concept.state_changed.v1 { from, to, evidence }`, `frontier.computed.v1`, `lesson.planned.v1`, `check.asked.v1 { concept_id, level }`, `check.answered.v1 { correct, level }`, `learning_record.saved.v1`, `card.viewed.v1`. Every mastery change is traceable to an event.

## 10. Eval additions (02)

Synthetic learners: scripted answer policies (always right, right below level 3, random, overconfident rater) run against a synthetic path of 20 concepts. Metrics: frontier correctness against the scripted knowledge 0.90; overconfident rating caught within two checks 0.95; level adaptation follows the rule 1.00; no check generated from unverified text 1.00; lesson coverage of the target concepts' required facts 0.85; learning record traceability 1.00; map state consistency with the event log 1.00.

## 11. Build

Phase 13, after 12 (vault), since learning records are pages: 13a Concept amendments, ConceptEdge, Mission, LearningPath, events; 13b Learning Planner with deterministic frontier and level rules; 13c Exercise levels and Tutor integration; 13d Map view; 13e research retriever profile; 13f learning records; 13g placement flow and Home summary. Acceptance per sub-phase from section 10 plus a full scripted session per synthetic learner.

## 12. Open questions

1. Prerequisite proposals for a brand new topic come from the model. Should the learner confirm them one by one, or accept the set and correct later? Proposal: accept the set, correct on the map by dragging an edge.
2. How much exploration counts as exposure. Three seconds of hover is a guess; measure on yourself first.
3. Spaced review: the state machine has `decayed` and `last_evidence_at`, so a scheduler is additive. When to build it depends on whether daily use shows decay mattering.
4. Whether a lesson board should be allowed to become an ordinary explore board after the lesson ends (keep mode learn with the record, or flip). Proposal: keep the mode, allow exploration on it anyway; the map reads both.
