# WorkMesh

WorkMesh is a docs-first project and task orchestration system for developers and coding agents.

Standalone distribution: [luis-lobo-ar/workmesh](https://github.com/luis-lobo-ar/workmesh). Clone that repository to build only WorkMesh and its required shared crates, or download Linux snapshot binaries from its Releases page. This directory remains the authoritative source; the standalone repository receives generated snapshots. The external export supplies its own root Cargo workspace, without creating another workspace inside Mesh.

`Mesh` is now the primary repository for WorkMesh development. Build, test, and evolve WorkMesh from the monorepo root at `/home/luis/dev/luislobo/mesh`, not from a standalone sibling clone.

It keeps planning state next to the code:
- active task and epic Markdown files under `tasks/`, including nested organization by customer, project, application/service, initiative, or feature
- repo-local context in `.workmesh/context.json`
- durable decisions in `.workmesh/truth/`
- global continuity in `~/.workmesh/`

By default, actionable tasks must include `Description`, `Acceptance Criteria`, and `Definition of Done`, and `Definition of Done` must be outcome-based. Those task-quality requirements are configurable in `.workmesh.toml` or global config when a repo wants a looser or stricter policy. Incomplete work should stay in `Draft` or `Needs Refinement` until it is ready for `To Do` or `In Progress`.

`workmesh next` and `workmesh ready` are leaf-first by default. They discover active task and epic files recursively under `tasks/`, return actionable work items such as tasks, bugs, incidents, and subtasks, and treat epics as planning containers. When a returned task has parent lineage, CLI and JSON surfaces include ancestry context so operators can see the larger stream without having container items compete as the next executable step.

Use directories for human navigation, not identity. Recommended layouts include `tasks/<product>/<initiative>/<feature>/` or, for client-heavy repos, `tasks/<customer>/<project>/<application-or-service>/<feature>/`. The task `id`, `kind`, `relationships.parent`, `dependencies`, `labels`, `phase`, and `status` front matter remain the source of truth, so agents can reorganize files without changing task identity. Archived tasks move under `archive/<yyyy>/<mm>/<status>/<relative-task-path>` so history is date-first while preserving the original customer/project/application/feature path.

Maintenance fixers can backfill missing task UIDs, remove broken dependencies, repair duplicate task IDs, and normalize drifted task filenames from task metadata.

## Install

Verify installed binaries:

```bash
workmesh --version
workmesh-mcp --version
```

Build standalone source (the same Cargo commands work from the Mesh root):

```bash
git clone https://github.com/luis-lobo-ar/workmesh.git
cd workmesh
cargo build -p workmesh --bin workmesh
cargo build -p workmesh-mcp --bin workmesh-mcp
```

CI/release artifact path:
- `.github/workflows/workmesh-build.yml` tests the WorkMesh package set from the root workspace.
- The workflow builds release artifacts for the installable operator binaries `workmesh` and `workmesh-mcp`.
- `workmesh-mcp-server` is a library crate exercised through tests and dependency builds; it is not an installed binary.

Codex MCP example:

```toml
[mcp_servers.workmesh]
command = "/usr/local/bin/workmesh-mcp"
args = []
```

## Skills

Canonical WorkMesh skills live under `skills/`.

Installed agent paths follow the shared Agent Skills layout:
- project scope: `.agents/skills/` for Codex and Cursor, `.claude/skills/` for Claude
- user scope: `~/.codex/skills/`, `~/.cursor/skills/`, and `~/.claude/skills/`

Each skill is self-contained and includes its referenced doctrine files inside its own skill root.

## Cross-Mesh References

Tasks can carry typed references to other mesh products without WorkMesh taking ownership of those systems. Store them in task front matter under `mesh_refs`:

```yaml
mesh_refs:
  boardmesh:
    sessions: [board-main]
    canvases: [canvas-api]
    objects: [svc-auth]
  guildmesh:
    rooms: [room-beta]
    threads: [thread-release]
  shipmesh:
    deployments: [deploy-beta-42]
    environments: [beta]
  testmesh:
    runs: [run-smoke-17]
    suites: [smoke]
  timemesh:
    workspaces: [platform]
    entries: [entry-123]
```

Current typed groups are:
- `boardmesh`: `sessions`, `canvases`, `objects`
- `guildmesh`: `rooms`, `threads`, `handoffs`
- `shipmesh`: `deployments`, `environments`
- `testmesh`: `runs`, `suites`
- `timemesh`: `workspaces`, `entries`

The canonical owner-scoped meanings of those foreign ids live in [../../share/doc/architecture/cross-product-contracts.html](../../share/doc/architecture/cross-product-contracts.html). `mesh_refs` is WorkMesh's product-local storage shape for preserving those external links; it does not make WorkMesh the owner of the referenced records.

WorkMesh preserves the raw front-matter value and also exposes a normalized top-level `mesh_refs` object in JSON export surfaces.

## Documentation

Primary documentation:
- [`docs/README.md`](docs/README.md)

Supporting references:
- [`docs/architecture.html`](docs/architecture.html)
- [`docs/reference/commands.html`](docs/reference/commands.html)
- [`docs/setup/run-modes-and-agent-mcp.html`](docs/setup/run-modes-and-agent-mcp.html)
- [`CHANGELOG.md`](CHANGELOG.md)
- [../../share/doc/architecture/agent-operating-environment.html](../../share/doc/architecture/agent-operating-environment.html)

Monorepo note:
- ecosystem-wide planning and active implementation tracking live at the repository root under `tasks/`
- product-local historical/project material may still exist under `opt/workmesh/`, but new cross-product work should be tracked from the monorepo root
- `opt/workmesh/workmesh/` is retained as WorkMesh-owned project/history content inside the monorepo, not as a separate repository or alternate source of truth

Agent-readable mirror:
- [`README.json`](README.json)
