# CI's on-demand runner

GitHub's free runners come first. One DigitalOcean droplet
(CPU-Optimized `c-16`: 16 vCPUs, 32 GB, $0.50 an hour) takes the overflow:
when a CI run that changes the engine or the server starts while GitHub's
runners are full (a Linux job has waited over a minute for one, or fewer
than 6 of the account's 60 are free; on 13 November at 04:10 UTC, 55 Linux jobs ran and 54 had waited over a minute), its two longest jobs go to the
droplet: chloedb-server's release binary (job `server-bin`, which the smoke
test, the Python and JavaScript packages and the guide's pages then
download) and chloedb-server's tests. Every other job, and every run while
GitHub has room, stays on GitHub's runners. The droplet exists only while
it is needed:

| what | where |
|---|---|
| made when a run overflows and no droplet is up, and the run waits up to 4 minutes for it | `ci_runner.py want`, ci.yml's job `runner` |
| destroyed once no CI run has been queued or running for 15 minutes; after 12 hours whatever happens (14 if a job still runs); after 11 minutes if no runner came up | `ci_runner.py reap`, `ci-runner.yml` (after every CI run, and every 15 minutes) |
| starts from the snapshot `ci-runner-base-<time>`: Ubuntu 24.04, Rust (ci.yml's `RUST_TOOLCHAIN`), the Actions runner | `provision.sh`, `ci_runner.py image`, `ci-runner.yml`'s job `image` (by hand, monthly, and when this folder changes on main) |
| keeps cargo's registry, the runners' work folders and the build folders on the volume `ci-runner-cache` (100 GB), attached to each new droplet | `boot.sh`, `target-dir.sh` |
| refuses a job of a pull request from a fork, or of another repository, before its first step | `job-started.sh`, the runner's job-started hook |

The droplet is destroyed rather than powered off because DigitalOcean bills a
powered-off droplet in full. When the droplet is off, busy with another
run's jobs, not set up, or does not come up in time, the jobs run on
`ubuntu-latest` as before, and the run's summary says which and why.

## Switching it on

It needs two repository secrets (Settings, Secrets and variables, Actions):

- `DIGITALOCEAN_TOKEN`: a DigitalOcean API token with write access. A
  custom-scoped token needs droplets, snapshots, block storage (volumes),
  images, tags and actions.
- `CI_RUNNER_ADMIN_TOKEN`: a fine-grained GitHub token for this repository
  only, with the permission **Administration: read and write**: it makes the
  runners' registration tokens and reads whether they are busy.

Then run the workflow "CI runner" by hand with `image` once, which builds
the first snapshot (about 15 minutes, a few cents).

Repository variables, all optional: `CI_RUNNER=off` sends every job back to
GitHub's runners; `CI_RUNNER_HOSTED_LIMIT` (default 60, about the jobs GitHub
ran at once for this account on 13 November); `CI_RUNNER_SIZE` (default `c-16`), `CI_RUNNER_REGION`
(`nyc1`, where `c-16` is offered; the snapshot and the volume must be there
too) and `CI_RUNNER_COUNT` (runners on the droplet, default 2).

## Cost

$0.50 for each hour the droplet exists, billed by the second, plus $10 a
month for the volume and about $0.10 for the snapshot. Since it only takes
the overflow, it is up only while GitHub's runners are full and for 15
minutes after the last CI run: on a quiet day nothing, on a day like
12 November (about 100 runs in 10 hours, several at once most of the time)
most of the busy hours. The run summary says, for each run, where its heavy
jobs went and why, which is the measure to watch.
