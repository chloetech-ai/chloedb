# GPU batch runs

A rented GPU machine runs the jobs in `jobs.txt` one after another and
publishes only their logs and result files, read-only over HTTP. Nothing on
it takes commands: `runner.sh` pulls this branch every minute and starts each
job it has not run yet, so a new job is a new line committed here.

- `cloud-init.sh`: what the machine runs at first boot (packages, this
  branch, the models, WikiText-2, then the runner and the file server).
- `runner.sh`: the loop. Each job writes `/root/out/<name>.log` and
  `<name>.json`, then `<name>.done` or `<name>.failed`; `status.txt` says
  what is running and the GPU's state.
- `jobs.txt`: one job a line, `name|ENV=... ENV=...|script.py`. The script
  gets `/root/out/<name>.json` as its argument.

The machine is deleted when the jobs are done (by whoever started it).
