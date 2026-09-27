# orbit

A tiny service, here so the documentation's recordings have something to
page through, pick from and search. Nothing in it runs.

## What it does

orbit watches a queue, hands each job to a worker and reports progress in the
terminal. Workers are plain functions; the scheduler decides how many run at
once, retries a job that fails, and gives up after three tries.

## Configuration

| Setting | Default | Meaning |
|---|---|---|
| `workers` | 4 | Jobs that run at once |
| `retries` | 3 | Tries before a job fails |
| `port` | 8080 | Where the status page listens |

## Running it

```bash
orbit --workers 8 queue.toml
```

The status page shows each worker, its job and how long it has run. A worker
that has run a job for more than a minute is marked slow; a job that failed
three times is listed with its last error.

## Components

The terminal side is built from rs-rich-interact components: a picker for
choosing a queue, a form for new jobs, a confirmation sheet before anything is
cancelled, and a pager for logs. Each component degrades to plain prompts
when the output is piped, so orbit works the same in CI.

## Scheduling

Jobs are taken oldest first. A job can ask to run after another; the
scheduler holds it until the other has finished. Two jobs that ask for the
same resource never run at once.

## Logs

Every job writes to its own log. The pager opens it at the end, follows it
while the job runs, and searches it with `/`.

## Licence

MIT.
