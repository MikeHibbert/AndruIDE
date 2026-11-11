# GitHub and git

AndruIDE uses the GitHub CLI's login (`gh auth login`) or `GITHUB_TOKEN`, one account at a time.

## What needs the user

```sh
andruide github inbox --json                          # review requests, assigned issues, their open PRs
andruide github repos --json                          # their repositories, favorites first (`favorite: true`)
andruide github repo acme/api --json                  # open issues and PRs, recent Actions runs, checkouts on their machines
andruide github issue acme/api#142 --json             # an issue or PR in full: description, comments; for PRs also checks and reviews
andruide github issue https://github.com/acme/api/pull/142 --json
```

`andruide github pr` is the same command as `github issue`.

## Checkouts on the machines

```sh
andruide checkouts --json                             # every git checkout AndruIDE found on the machines
andruide checkouts --repo acme/api --json             # just this repository's
andruide checkouts --machine beelink --json
andruide checkouts --machine beelink --path '~/code/api' --git pull --json   # fetch, pull (fast-forward only) or push there
```

Each checkout has its `machineId`, `path`, `repo`, `branch`, `upstream`, `ahead`, `behind` and `dirty` (uncommitted files). Run `--git` only when the user asked.

To work on an issue or review a PR there, see [sessions and boxes](sessions-and-boxes.md#start-one-on-an-issue-or-a-pull-request).

## Git on a machine

```sh
andruide git status beelink --json
andruide git setup beelink --identity --credentials --json
```

`git status` says whether an agent there can commit (`can_commit`) and how pushes reach GitHub (`push`: `gh`, `other` or `none`). `git setup --identity` sets git's name and email from the user's GitHub account; `--credentials` makes git push with the machine's GitHub CLI login (`gh auth setup-git`). It changes the machine's global git config: only when asked.

## Accounts and favorites

```sh
andruide github accounts --json                       # who AndruIDE uses (`using`), the alternatives, `scopes`, `orgs`
andruide github use work-login --json                 # switch accounts
andruide github use --machine work-macbook --json     # use that machine's gh login
andruide github favorite acme/api --json              # star it in AndruIDE (--remove to unstar)
```

Switching changes whose repositories and runners AndruIDE sees. When watching organizations needs more permissions, `github accounts` has a `grant_command`: pass it to the user verbatim. Switch accounts or change favorites only when asked.
