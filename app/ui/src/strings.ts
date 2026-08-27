/**
 * Every string the user reads, in one place.
 *
 * Two reasons it is a file rather than a habit. BN-024: the product name lives
 * in one constant, so changing it is one edit. BN-030: the house style lint in
 * `crates/tessera-style` reads TypeScript only from a file with this name,
 * because guessing which string in a general module is copy produced a six in
 * six false positive rate and a lint nobody would keep.
 *
 * So copy goes here and the lint checks it. `cargo test -p tessera-style` is
 * what enforces `HANDOFF.md` section 7 on everything below.
 *
 * The name is also declared in `app/src-tauri/tauri.conf.json` as the name the
 * operating system shows. That one is packaging metadata and cannot read a
 * TypeScript constant; the window title below is set from here at startup, so
 * the static title in `index.html` is only what shows before the script runs.
 */

export const PRODUCT_NAME = 'Tessera';

export const COPY = {
  /** Shown in the composer when no core is behind the page. */
  /** The composer, before Learn is on. index.html carries it statically too. */
  askSomething: 'Ask something',

  askOffline: `Open ${PRODUCT_NAME} to ask a question`,

  /** The error a call gets when the page is open outside the desktop app. */
  notConnected: `This page is not connected to a core. Open ${PRODUCT_NAME} to ask a question.`,

  // ------------------------------------------------------- the card header --

  /** A follow-up card has no anchor to name, so the kind is the title. */
  followTitle: 'Follow-up',
  /** Doc 09 section 4: the confidence dot before the Verifier has run. */
  unverified: 'Unverified',
  /** Hover on the dot once it has. The number follows this word. */
  confidence: 'Confidence',
  /** Hover on the model alias, doc 09 section 4. */
  rerunAs: 'Rerun as…',
  /** Doc 09 section 5's Rerun verb on a card. */
  rerunCard: 'Check this card again',
  /** The flag chip opens the card list of flags rather than leaving a count. */
  openFlags: 'Show what was flagged',
  closeFlags: 'Hide what was flagged',

  // --------------------------------------------------------- the card body --

  /** Read aloud while a card is running and its stage list is still empty. */
  working: 'Working',
  cardFailed: 'This card did not finish. Rerun it, or open how this was built.',
  keyFindings: 'Key findings',
  howBuilt: 'How this was built',
  /** The count follows this word. */
  sources: 'Sources',
  /** The marker on a citation whose source moved under the card. Doc 07 B3. */
  staleTag: 'stale',
  /** Doc 07 section B8.3: a block that fails is hidden, never removed. */
  blockHidden: 'Hidden after review.',
  blockHiddenUnexplained: 'A flag covers this block.',

  // ------------------------------------------------------ the card actions --

  askFollowUp: 'Ask a follow-up',
  sendFollowUp: 'Send follow-up',

  // ------------------------------------------- the two branch popovers, 09.3 --

  /** Offered on a span selected inside a card body. */
  askAboutThis: 'Ask about this',
  /** Offered on a block of a visual, which carries an exact pointer. */
  investigateBlock: 'Investigate this further',

  // ------------------------------------------ how this was built, doc 09.4 --

  builtRouted: 'Routed',
  routedPlanned: 'to the Planner',
  routedDirect: 'straight to the answer',
  builtPlanned: 'Planned',
  builtSubQuestions: 'sub questions',
  builtCoverage: 'coverage',
  builtPassages: 'passages',
  builtVisual: 'Visual',
  builtDrawn: 'drawn',
  builtDeclined: 'declined',
  builtVerified: 'Verified',
  /**
   * What the Verifier did, counted from the event rather than characterised.
   * The first version of this row said "checked against its sources" on every
   * card, including a fast one, which cites nothing and had no sources to check.
   */
  builtRulesPassed: 'rules passed',
  builtRulesFlagged: 'flagged',
  builtRulesSkipped: 'skipped',
  builtCitationsSupported: 'citations supported',
  builtOf: 'of',
  builtNoChecks: 'no rule applied',
  builtCalls: 'model calls',
  builtTokens: 'tokens',
  /** A card whose events say nothing at all, which is a card that never ran. */
  builtNothing: 'No events were recorded for this card.',
  builtFailed: 'The build trail could not be read.',

  // ------------------------------------------------------------- the shell --

  modeStarting: 'Starting',
  modeLive: 'Live',
  modeWorking: 'Working',
  modeOffline: 'Offline',

  // --------------------------------- the board as a document, doc 11 section 10 --

  readingOpen: 'Read as a document',
  readingClose: 'Back to the board',
  readingEmpty: 'This board has no cards yet.',
  readingNoAnswer: 'This card has not answered yet.',
  readingFollowUp: 'a follow-up',
  readingBranchFrom: 'branched from',
  readingBranchFromBlock: 'branched from the block at',
  readingVisualType: 'The visual is a',
  readingFlags: 'Flags',

  // ---------------------------------------------- the Reader, doc 07 part A --

  /** Doc 07 section A11: a read card says where it came from. */
  readFromImage: 'Read from an image',
  readFailed: 'That image could not be read.',
  readIllegible: 'Could not read this image.',

  /** Doc 15 section 2: a prior card is context, and this names which. */
  builtBuildsOn: 'Builds on',

  // -------------------------------------------- First run, doc 11 sec 6 --

  setupTitle: 'Set up Tessera',
  setupLede: 'Three steps. The third one is optional.',
  setupLoading: 'Reading what is already set up',
  setupWorking: 'Working',
  setupDone: 'Start asking questions',
  setupNeedsKey: 'Add a model key first, so the first card has something to answer it.',

  setupPackTitle: 'Choose a doctrine pack',
  setupPackNote: 'The pack decides which sources outrank which and what a card must never say without one. You can change it later.',

  setupKeyTitle: 'Add a model key',
  setupKeyNote: 'The key goes into this machine\u2019s keychain. It is never written to a file and never leaves except to the provider you chose.',
  setupKeyLabel: 'Paste the key for',
  setupKeyPlaceholder: 'The key from your provider',
  setupKeySave: 'Save to keychain',
  setupKeyPresent: 'A key is in the keychain for this alias.',

  setupFolderTitle: 'Watch a folder',
  setupFolderNote: 'Optional. Documents in a watched folder can be cited. Nothing is uploaded unless you ask for provider embeddings.',
  setupFolderPath: 'Full path to the folder',
  setupFolderLabel: 'What to call it',
  setupFolderSensitive: 'Sensitive: keep this folder\u2019s text on this machine',
  setupFolderAdd: 'Watch this folder',
  setupFolderAdded: 'Watching',
  /**
   * Doc 05 section 11: adding a folder reads it, so the step says what the read
   * found. A folder that indexed nothing looks exactly like one that indexed
   * everything until the count is on the screen.
   */
  setupFolderIndexed: 'Documents indexed:',
  setupFolderUnreadable: 'Files this reader could not open:',

  // ------------------------------------------------ Learn mode, doc 14 --

  /** Doc 14 section 4: a Learn toggle left of the depth selector. */
  learnToggle: 'Learn',
  learnPlaceholder: 'What do you want to learn?',
  learnTopic: 'Learning about',
  learnAsk: 'Ask the tutor',
  learnBuild: 'Build these cards',
  learnSkipIntake: 'Just build it',
  learnNext: 'Open the next card',
  learnAnother: 'Another question',
  learnStop: 'End the session',
  learnRight: 'Right.',
  learnWrong: 'Not quite.',
  learnGotItRight: 'Got it right',
  learnGotItWrong: 'Got it wrong',
  learnThinking: 'Thinking',
  learnNone: 'No session on this board.',
  learnFailed: 'The tutor could not answer.',
  /** Doc 14 section 3.9's header stage label, in words. */
  learnStageIdle: 'Ready',
  learnStageIntake: 'Getting to know you',
  learnStageBuilding: 'Planning the board',
  learnStageReading: 'Reading',
  learnStageChecking: 'Checking understanding',
  learnStageEnded: 'Finished',
  learnEnded: 'Session over.',

  // ------------------------------------------------- the exercise, doc 08 --

  /** Doc 09 section 4 puts this in the toolbar. */
  exerciseCheck: 'Check understanding',
  exerciseTitle: 'Check your understanding',
  exerciseSubmit: 'Check my answers',
  exerciseScored: 'You got',
  exerciseDone: 'Close',
  exerciseOpenCard: 'Open the card',
  /** Doc 08 section 11: a wrong item is reported for pack maintenance. */
  exerciseReport: 'Report this question',
  exerciseReported: 'Reported. It goes to whoever maintains the pack.',
  exerciseNone: 'No exercise yet.',
  exerciseNothingToCheck:
    'No card on this board has been checked against a source yet, so there is nothing to test.',
  exerciseDropped: 'Some questions were dropped because they could not be traced to a card.',
  exerciseFailed: 'That exercise could not be generated.',
  exerciseWorking: 'Writing the questions',

  // ------------------------------------------------- the rail, doc 11 section 5 --

  railHome: 'Home',
  railFlags: 'Flags',
  railLibrary: 'Library',
  railProfile: 'Profile',

  // ------------------------------------------------------ ages on queue rows --

  agoNow: 'just now',
  agoMinutes: 'm',
  agoHours: 'h',
  agoDays: 'd',
  agoMonths: 'mo',
  agoUnknown: 'unknown',

  // -------------------------------------------------------- Home, doc 09.3 --

  homeFilterLabel: 'Which boards',
  homeActive: 'Boards',
  /** Doc 09 open question 1, adopted by doc 11: Trash is a filter, not a page. */
  homeTrashed: 'Trash',
  homeCreate: 'New board',
  homeOpen: 'Open',
  homeTrash: 'Move to Trash',
  homeRestore: 'Restore',
  homePurge: 'Delete for good',
  homeCards: 'cards',
  homeOpenFlags: 'Open flags on this board',
  homeNoBoards: 'No boards yet. Ask a question to start one.',
  homeNoTrash: 'Trash is empty.',

  // ------------------------------------------------------- Flags, doc 09.6 --

  flagsNone: 'No open flags. Every card on this profile has been read or cleared.',
  flagsOpen: 'Open',
  flagsAccept: 'Accept',
  flagsDismiss: 'Dismiss',
  flagsRerun: 'Rerun',
  flagsSelectRow: 'Select this flag',
  flagsSelectBoard: 'Select all on this board',
  flagsBulkLabel: 'Decide the selected flags',
  flagsSelected: 'selected',
  flagsClear: 'Clear',
  /** Doc 09 section 6: bulk Dismiss takes a second click with the count shown. */
  flagsDismissConfirm: 'Dismiss',
  flagsFailed: 'That decision was not recorded.',

  // ----------------------------------------------------- Library, doc 09.9 --

  libraryTabsLabel: 'Library tabs',
  librarySources: 'Sources',
  libraryConcepts: 'Concepts',
  libraryOpen: 'Open',
  libraryAsk: 'Ask about this',
  libraryAccept: 'Accept',
  libraryDismiss: 'Dismiss',
  libraryRemove: 'Remove',
  libraryCitedOn: 'cards cite it',
  libraryVerified: 'checked',
  libraryNeverVerified: 'never',
  libraryTrustRank: 'Trust rank from the doctrine pack',
  libraryNoIssuer: 'no issuer recorded',
  libraryLinks: 'links',
  libraryNoDefinition: 'No definition yet.',
  libraryNoSources: 'No sources yet. A deep question retrieves the first ones.',
  libraryNoConcepts: 'No concepts yet. They are proposed from the entities a card names.',

  // ---------------------------------------------------- Profile, doc 11.6 --

  profileTabsLabel: 'Profile pages',
  profileContext: 'Context',
  profileModels: 'Models',
  profileRetrievers: 'Retrievers',
  /**
   * Doc 10 section 9. The verb says what it does to the board rather than to
   * the pack, because what a person is deciding is whether to have these cards
   * judged again.
   */
  boardPackUpdate: 'Update pack and check these cards again',
  boardPackUpdated: 'Judged again under',
  boardPackUpdateFailed: 'The pack update did not finish. The board still names the version it had.',

  /**
   * Doc 16 section 3.2's ninth verb, and doc 16 section 7 point 1's vocabulary:
   * a sticky is the thing on the board and a page is the thing in the vault.
   */
  saveAsPage: 'Keep as a page',
  savedAsPage: 'In my pages',
  saveFailed: 'That card was not saved.',

  profileDoctrine: 'Doctrine',
  /** Doc 10 section 9: a pack is data, and importing one is reading a file. */
  profilePackImport: 'Import a doctrine pack',
  profilePackPath: 'Full path to the pack file',
  profilePackImported: 'Imported',
  profilePackBuiltIn: 'Ships with the app',
  profilePackActive: 'Active',
  profilePackUse: 'Use this pack',
  profilePackImportNote: 'A pack sets the audiences, the source ranking and the flag rules. Importing one adds it to this profile; switching to it is a separate step.',
  profilePackUnread: 'These pack files in this profile folder did not load:',
  profileDiagnostics: 'Diagnostics',
  profileId: 'Profile',
  profileProvider: 'Provider',
  profileActivePack: 'Active pack',
  profileKeyRef: 'key',
  profileKeySaved: 'key saved',
  profileKeyMissing: 'no key',
  profileKeyAdd: 'Add key',
  profileKeyReplace: 'Replace key',
  profileKeyPrompt: 'Paste the key for',
  profileKeySavedToast: 'The key went to the keychain.',
  profileKeyFailed: 'The keychain would not take that key.',
  /** Doc 10 section 8, said where a person is about to paste a secret. */
  profileKeyNotice: 'Keys are held by the operating system keychain. Nothing writes one to a file, a log or a bundle.',
  profileConfigured: 'configured',
  profileUnconfigured: 'not configured',
  profileOnByDefault: 'on by default',
  profileBoards: 'Boards',
  profileTrashed: 'In Trash',
  profileCards: 'Cards',
  profileOpenFlags: 'Open flags',
  profileSources: 'Sources',
  profileConcepts: 'Concepts',
  profileEvents: 'Events',
  profileNoAliases: 'This policy names no model aliases.',
  profileNoRetrievers: 'The active pack lists no retrievers.',
  profileNoDiagnostics: 'The core reported no counts.',
  profileUnread: 'The profile could not be read.',

  // -------------------------------------------------------------- failures --

  /** Doc 11 section 9: say what happened and how to fix it. */
  askFailed: 'That card did not finish.',
  coreSilent: 'The core did not answer.',
  rerunFailed: 'That card could not be checked again.',
  renameFailed: 'That board could not be renamed.',
  boardVerbFailed: 'That board could not be changed.',
  pageUnread: 'That page could not be read.',
  conceptFailed: 'That concept could not be decided.',
} as const;
