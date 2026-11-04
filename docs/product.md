# Product definition and user journeys

Status: v0.3.0 product contract. Implementation and actual validation are tracked in [acceptance.md](acceptance.md), not inferred from this design.

## Purpose

RightWake brings new work events back to the existing agent conversation that owns the work. Its primary user is a developer who has several active local Codex threads and receives follow-up questions or changes through Linear, GitHub, or GitLab.

The activation outcome is one verified real event reaching its correct existing thread and producing an inspectable local result. The five-minute activation target begins **after** the user has the necessary provider permissions, credentials, reachable ingress, and a supported host. It is a target to measure, not an established performance claim.

The user's later requirements override the original prompt's optional automatic thread creation: RightWake must not create or fork a work thread when ownership is missing. The former “create once, then reuse” scenario becomes “claim an existing thread once, then reuse.” This change is explicit in the acceptance ledger.

## Scope and boundaries

| Area | Product requirement | Boundary |
| --- | --- | --- |
| Sources | Linear, GitHub, GitLab, and configured Generic Webhook | Minimum event and reply types must be listed individually; Generic is not evidence of native-provider completion |
| Execution | Existing Codex App Server thread and configured GenericCommand | A thread ID alone does not establish access, history, safe concurrency, or tool permissions |
| Routing | Explicit ownership first, bounded semantic evaluation when justified | Similar titles, mentions, and high model confidence do not create ownership or authorization |
| Context | Durable thread directory and small evidence-backed cards | No global conversation that silently absorbs all chats; original history remains at its host |
| Delivery | Durable queue, thread serialization, observable uncertain outcomes | A receipt acknowledgement is not proof of agent completion or provider publication |
| Replies | Draft by default, exact independent approval before a send | The same agent cannot approve its own draft by invoking an MCP tool |
| Distribution | Native CLI and repository Codex plugin | Public plugin-directory acceptance is a separate process |
| Deployment | Local, self-hosted relay plus local executor, self-hosted cloud | No assumed hosted relay and no automatic migration of local chat history |

Slack support, a general workflow canvas, organizational task management, and broad multi-agent orchestration are outside this release. The full original requirements that remain incomplete are retained as pending items, not reclassified as optional.

## Information the user manages

| Concept | User question | Required stored information |
| --- | --- | --- |
| Source | Where do events come from? | Provider, instance/account identity, permitted scope, secret reference, trigger policy, enabled state |
| Work resource | Which work is this about? | Canonical issue/PR/MR identity, discussion identity where available, original link |
| Destination | Which existing conversation continues? | Host ID, thread ID, expected work directory, verified capabilities |
| Binding | Who owns this resource? | Source/resource or discussion key, destination, version, enabled state, origin |
| Context card | Why is this thread a candidate? | Goal, summary, owned versus referenced resources, evidence, open questions, revision, coverage |
| Pending work | Why has this not run? | Event, route evidence, queue state, reason, attempt outcome, next action |
| Draft | What would be sent? | Exact body, reply location, identity, source/binding/draft versions, approval and send outcome |

## First installation: Codex plugin

1. Add the repository marketplace and install RightWake in a supported local Codex host.
2. The setup skill locates the native binary, checks its version, reads `schema`, and runs `doctor`. If the user requested installation and the binary is missing, it uses the pinned release installer.
3. Connect one source and select its actual workspace/repository/project scope. Ask for a choice only when it cannot be established from trusted configuration. Credentials remain outside chat as environment or supported secret references.
4. Choose an existing work destination. A newly opened plugin setup conversation is not automatically that destination. If the current thread's identity cannot be established through a public host interface, present known existing candidates or request the intended destination; never guess from the title.
5. Show the concrete binding once: resource, host, thread, and draft behavior. Save the authorized binding and read it back.
6. Explicitly start the receiver or user service. Configure direct webhook ingress or a user-controlled relay. An unreachable local endpoint is a setup blocker, not a successful connection.
7. Trigger one harmless authorized event. Show “received,” then the actual delivery outcome, then the local draft. Leave any independent sending approval for the exact draft.

Suggested dialogue:

> “I will connect this project's events to its existing work thread and keep replies as drafts. I found the source and local host. This resource is currently unbound; the two plausible threads are A and B. A owns the issue, while B only references it. Shall I record A as the owner?”

If ownership is already explicit, do not ask this question repeatedly. If required provider setup needs an administrator, identify the exact missing permission and leave the completed local configuration intact.

## First installation: standalone CLI

1. Install the native executable; inspect `--version`.
2. Run idempotent `setup`, `schema`, and `doctor`.
3. Configure a source with credential references and an execution host. Only Codex hosts need a Codex installation; GenericCommand does not.
4. Bind the canonical resource to an existing thread/continuation identifier, then inspect `bindings`.
5. Start `serve` or explicitly install the user service. Configure reachable webhook ingress.
6. Send the authorized test event and inspect events, jobs, and drafts.

The installed version's machine-readable contract is authoritative for flag names. Agent helpers discover it instead of carrying stale copies. [Installation](installation.md) documents the native platform and service requirements.

## Daily journeys

| User intent | Product behavior | Inspection/action surface |
| --- | --- | --- |
| “Continue this issue here” | Resolve a real current work thread; bind the exact resource | `bind`, then `bindings` |
| A mention arrives for a bound issue | Validate, deduplicate, queue to its existing destination | `events show`, `jobs show` |
| Watch comments on my Linear issues and newly assigned tasks | Maintain observed assignee scope from signed Issue changes and a verified baseline; comments need no mention; preserve existing thread routing | `source linear-import`, `source linear-state`, events and bindings |
| An issue is transferred away before queued work starts | Recheck its observed assignment and hold work outside the personal scope | `outside_assignment` and the recorded Issue revision |
| A message adds no actionable work | Update relevant state; do not start an unnecessary model turn | Event skip reason |
| Two issues share one thread | Keep both bindings and process the thread serially | Jobs ordered by destination |
| The user changes windows | Preserve the durable destination | Binding remains identical |
| An unknown issue resembles several tasks | Explain the best candidates; record one justified claim | Context search, binding action |
| A discussion has distinct ownership | Use its explicit discussion binding before the parent issue | Binding scope is visible |
| Move an issue to another existing thread | Show old and new destinations; version the change; invalidate stale queued work | `bind`, jobs and drafts |
| The original thread is busy or offline | Wait on that same destination | Job reason and retry/reconnect state |
| Review a response | Show exact text, source, destination, identity, and versions | `drafts show`, `drafts export` |
| Stop automation | Pause the intended scope or stop the service without changing ownership | Status and explicit service control |

## Ambiguity and semantic routing

The routing decision has two independent questions: “Is this new actionable work?” and “Which existing thread is authorized to own it?” A semantic router can help with both, but cannot rewrite access policy.

Example: a Linear comment on issue A says “this may be related to B.” A is bound to thread A; B is bound to thread B. The comment stays with A unless there is a separately trusted explicit discussion binding or a user-authorized reassignment. Mentioning B is a relation, not a routing command.

Example: issue A has two subdiscussions with explicitly separate owners. An exact discussion binding wins; an unspecified new discussion inherits the issue binding. A semantic suggestion to move it is presented as a candidate rather than silently changing ownership.

Example: a new issue has no binding, but a context card claims it as an owned resource with recorded user/host evidence. The runtime may make an automatic assignment only if the evidence meets configured policy, source scope, uniqueness, and host capability checks. Two candidate cards, stale evidence, or a title-only match require one explicit claim. No candidate means pending work, not a new conversation.

## Review and recovery

Draft review must expose the exact destination and sending identity together with the body. Edits to the body, source event revision, destination, or binding invalidate an old approval. A later event can stale an earlier draft. An external timeout after sending is an uncertain outcome that must be reconciled before retrying.

State language and permitted recovery actions are in [states.md](states.md). Read-only inspection is immediate. Actions that change ownership or publish material are concrete and scoped; routine inspection must not become a repeated confirmation loop.

## Success measures

Measure actual first-event setup time after prerequisites are ready, correctly continued existing-thread events, unresolved ownership rate, unnecessary wake-ups, repeat user claims, queue-to-draft latency, and duplicate external replies. Track blocked host capability separately from product errors. Do not invent target pass rates before representative acceptance tests exist.

## Related products

[Claude Tag](https://claude.com/resources/articles/claude-tag-now-reads-even-more-of-the-room) uses channel context to decide whether to respond, start deeper work, route a message to work already in progress, or stay silent. That validates a similar user need for semantic participation and workstream continuity. Its [documented Slack memory and identity boundaries](https://support.claude.com/en/articles/15594475-what-is-claude-tag) do not establish an ability to resume arbitrary local Codex threads.

RightWake's specific scope is an external work event returning to an explicitly owned existing **local host and thread**, with durable assignment and visible delivery state. It does not attempt to reproduce Claude Tag's shared Slack workspace.

[OpenAI MCP Events](https://developers.openai.com/plugins/build/mcp-events) is a relevant native subscription path for its supported cloud conversation surfaces. A future adapter may reuse it where the target surface fits. Its existence alone is not proof that local Codex thread continuity, arbitrary existing-thread routing, or a provider's event capability is available.
