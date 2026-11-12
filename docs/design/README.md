# Design documents

Proposals for changes that are hard to undo: the on-disk format, the shard
protocol, replication and authentication (the one-way doors in
[`.github/CODEOWNERS`](../../.github/CODEOWNERS)). Each one says what
exists, what it proposes, the alternatives it turned down, the formats and
calls it adds, how it fails, how a running cluster moves to it, and the
questions its owner has to answer before work starts.

A design here is a proposal until its status says it was accepted. The code
and [`chloedb/DESIGN.md`](../../chloedb/DESIGN.md) describe what is built.

| document | release | status |
|---|---|---|
| [A cluster you can trust](0.6.0-cluster.md): a consensus home for the cluster's metadata, a control plane apart from the query path, shard identity, read levels, point-in-time recovery, and a Jepsen-style test | 0.6.0 | proposal, waiting for the owner's review |
| [A file per agent, on object storage](0.7.0-file-per-agent.md): a database per tenant or agent kept on S3 and opened on demand, search served straight from object storage (stateless nodes, or a browser with no server at all), and a blackboard for agent fleets | 0.7.0 | proposal, waiting for the owner's review |

Both come from the audit of 12 November 2026 and the release plan proposed
with it. Neither version's scope is decided until the owner accepts the plan.
