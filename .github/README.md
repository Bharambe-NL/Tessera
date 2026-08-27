# What runs, when, and what it costs

Three workflows. Two of them are free and one of them is not, and that is the
line the split is drawn on.

## `checks.yml`, on every push

Everything that costs nothing:

| Job | What it proves |
| --- | --- |
| `rust` | The workspace formats, lints clean at `-D warnings`, and its tests pass with no crate excluded. |
| `ui` | The UI typechecks and builds, and the Playwright suite drives the built product against a real core, so a verb that is registered and unreachable fails here. |
| `generator` | The guard tests hold: every metric is gated, exempted or named a readout, and one with nothing to measure reports n/a rather than zero. |
| `corpus and sweep` | The corpus regenerates from its seed, the twenty synthetic boards round trip through a bundle, and the grounded sweep scores at or above every threshold. |

The corpus is not committed. It is a few megabytes of generated documents and it
is reproducible from seed 42, which is the whole point of a seed.

`gen score` exits non-zero when a measured metric is below its threshold, so
that step is the gate rather than a report printed beside one.

## `eval.yml`, when a person asks

The sweep against real providers. Doc 12 phase 11 asks for a nightly, and the
schedule is written and commented out: a job that bills an account every night
is a decision somebody makes on purpose, not a default that arrives with a merge.

Run it from the Actions tab. It takes the number of questions, the pack, and the
bulk provider, because the full 400 question set against a frontier model is real
money and 40 questions usually answers the question you had.

### Keys

Repository secrets, named by provider: `TESSERA_KEY_MOONSHOT`,
`TESSERA_KEY_ANTHROPIC`, `TESSERA_KEY_OPENAI`.

They reach the process as environment variables for one step. A runner has no OS
keychain, so this is the only path that works, and `TESSERA_CI` has to be set
before the eval will read one: a keychain that is merely locked on someone's
laptop must never fall through to an environment holding nothing.

A key is never an argument. An argument shows up in `ps`, in a crash dump, and in
the runner's own echo of the command it ran.

## `release.yml`, on a tag

Builds the msi and the dmg through `tauri-action`, runs the workspace tests
first, and opens a draft release so a person looks at what was produced before
anyone can download it.

Signing is conditional on the certificates being loaded as secrets, and the job
prints which build it made. An unsigned build that claimed to be signed is worse
than one that says it is not: the person finds out from Gatekeeper instead, by
which point they are holding a file nobody warned them about.
