# CI's on-demand runner

One DigitalOcean droplet (CPU-Optimized `c-16`: 16 vCPUs, 32 GB, $0.50 an
hour) runs the two longest jobs of a CI run that changes the engine or the
server: chloedb-server's release binary (job `server-bin`, which the smoke
test, the Python and JavaScript packages and the guide's pages then
download) and chloedb-server's tests. It exists only while CI works:

| what | where |
|---|---|
| made when a run needs it, and the run waits up to 4 minutes for it | `ci_runner.py want`, ci.yml's job `runner` |
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
GitHub's runners; `CI_RUNNER_SIZE` (default `c-16`), `CI_RUNNER_REGION`
(`nyc1`, where `c-16` is offered; the snapshot and the volume must be there
too) and `CI_RUNNER_COUNT` (runners on the droplet, default 2).

## Cost

$0.50 for each hour the droplet exists, billed by the second, plus $10 a
month for the volume and about $0.10 for the snapshot. CI ran 1,000 times
from 30 October to 13 November (about 77 runs a day). On two busy stretches
of those days the droplet would have been up for most of the time (10.8 of
10.6 hours from 12 November 17:00 UTC, 14.5 of 18.3 hours from 8 November
14:00 UTC), counting runs longer than 5 minutes and the 15 idle minutes after
each; at that pace it costs $5 to $10 a day, $150 to $300 a month, against
$336 for a droplet that never stops. Quieter days, nights and weekends cost
next to nothing.
