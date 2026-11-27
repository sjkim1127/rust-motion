# Free-model autopilot

OpenCode Zen free models file small issues, draft fixes, and review and merge them in GitHub Actions. No API key or repository secret is used.

| Step | Workflow | What happens |
|---|---|---|
| 1 | `free-model-triage.yml` | Every 6 hours a model looks for one small defect and files it (`agent:free`, `agent:auto`). |
| 2 | `free-model-draft.yml` | A model edits a checkout to fix the issue; the guard checks the edits and a PR is opened (`agent:free-draft`). |
| 3 | `free-model-merge.yml` | The merge guard checks the patch, a different model reviews it, CI runs, and the reviewed commit is squash-merged. |

## Switch

Everything unattended is off unless the repository variable `AGENT_AUTOPILOT` is `on`:

```bash
gh variable set AGENT_AUTOPILOT --body on
```

To stop it:

```bash
gh variable set AGENT_AUTOPILOT --body off
```

With the switch off, triage and merge do nothing, and an issue labelled `agent:free` still gets a draft PR for a person to review. Runs already in progress keep the value they started with; cancel them in the Actions tab.

## What waits for a person

A PR is labelled `agent:needs-human` and left open when:

- someone other than the owner or GitHub Actions wrote the issue;
- the patch deletes a file, changes existing test code or removes an assertion, adds a link, or is over 60,000 bytes;
- added Rust code switches a check off (`#[ignore]`, `#[cfg]`, `#[allow]`), uses `unsafe` or `extern`, or reaches files, processes, the environment or the network;
- the reviewing model rejects it, or none returns a usable verdict;
- CI fails, or the branch cannot be merged.

Draft limits also apply: at most 12 files and 400 changed lines, and no workflow, script, manifest, lock, toolchain or agent instruction file (`scripts/ci/free_model_draft.py`).

Triage files nothing while 5 `agent:auto` issues or 5 `agent:free-draft` PRs are open. An `agent:auto` issue that no model can draft is closed as not planned.

## Limits of the safeguards

The guard is a list of patterns and the reviewer is a free model, so a wrong change that compiles and passes the tests can reach `main`. CI is dispatched on `main` after each merge. Read the merged `agent:free-draft` PRs from time to time, and before a release.
