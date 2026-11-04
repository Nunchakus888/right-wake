# States and recovery

This is the user-facing state vocabulary. Runtime JSON state strings are defined by the installed `right-wake --json schema` and record output; the table describes their meaning and required next action rather than promising every UI affordance is already implemented.

| User-visible state | Condition | Explanation | Primary action | Secondary action | Automatic recovery |
| --- | --- | --- | --- | --- | --- |
| 已接收，等待处理 / Received | Event durably committed, work not submitted | The receiver saved the event; the agent has not completed it | View its route/job | Pause processing | On normal queue progress |
| 等待归属 / Needs assignment | No trusted destination | This work has no established owner | Choose an existing thread | Inspect candidates | Only if trustworthy new binding evidence arrives |
| 多个候选 / Ambiguous | Several plausible owners | Similar context does not establish a unique owner | Claim once with evidence | Leave pending | No silent guess |
| 等待设备上线 / Host offline | Bound host unreachable | Work remains assigned to its original thread | Restore this host/connection | Inspect host diagnostics | On an authenticated supported reconnect |
| 等待会话完成 / Thread busy | Bound thread has an active turn | Work will not interrupt the current turn | Wait for completion | Inspect queue | On a supported completion/idle signal |
| 已暂停 / Paused | Global/source/binding/host pause applies | Processing was intentionally stopped | Resume the same scope | View pending work | No |
| 会话已归档 / Archived | Target is archived | RightWake will not replace its destination | Restore it if intended | Explicitly rebind | No |
| 找不到会话 / Missing thread | Public host cannot find it | The original target could not be resolved | Verify the correct host/thread | Explicitly choose another existing target | No blind fallback |
| 工作目录失效 / Invalid work directory | Expected directory missing or different | The target's execution context has changed | Restore/verify the directory | Update the authorized binding | No |
| 宿主能力不支持 / Unsupported host | Safe continuation/concurrency unavailable | The selected host cannot establish this required capability | Read the specific compatibility gap | Use a supported existing host | After verified capability change |
| 等待审批 / Approval needed | Host or external action needs approval | A new permission is required for a concrete action | Review it through the trusted surface | Leave pending | No model-granted approval |
| 处理中 / Running | Host accepted a turn and has not completed | Work is running in the recorded thread | Inspect turn status | Stop through the supported host | On terminal host result |
| 已生成草稿 / Draft ready | Local proposed reply persisted | The platform has not received this reply | Review exact text and destination | Export or leave pending | No automatic send |
| 无需回复 / No reply needed | Policy/model concluded no useful reply | Relevant state was recorded without posting | Inspect reason | Re-evaluate explicitly | No unsolicited repeat |
| 草稿已过期 / Stale draft | Source, body, target, identity, or binding changed | The previous review no longer matches the current action | Compare changes and refresh the draft | Leave pending | Requires a new applicable review |
| 发送中 / Sending | Durable provider attempt in progress | The provider outcome is not yet known | Wait for the response | Inspect attempt details | On a known outcome |
| 已发送 / Sent | Provider success and reply identity persisted | The reply is recorded at the exact destination | Open provider link | Inspect audit record | Terminal |
| 处理结果待核对 / Unknown execution | Host may have accepted work before connection/result loss | Retrying could duplicate a turn | Reconcile original dispatch/turn | Leave blocked | No blind replay |
| 发送结果待核对 / Unknown send | Provider might have accepted the reply | Retrying could duplicate a comment | Inspect provider/readback evidence | Leave blocked | No blind replay |
| 授权失效 / Authentication failed | Credential missing, expired, or revoked | This source/host cannot authenticate | Restore the named credential reference | Re-run doctor | After credentials are corrected |
| 缺少平台权限 / Permission missing | Authenticated identity lacks required access | Platform setup or operation needs a specific permission | Ask the relevant owner for that permission | Reduce configured scope | After permission change |
| Webhook 未连通 / Ingress unreachable | Provider cannot reach/verify endpoint | A running local process is not a reachable webhook | Check URL, TLS, proxy, and secret | Use configured relay | After ingress correction |
| 存储已满 / Storage admission blocked | Durable write or configured limit failed | Event receipt cannot be acknowledged safely | Restore storage capacity | Inspect limits/backlog | Only when durable admission succeeds |
| 需要处理 / Retry exhausted | A known-safe operation repeatedly failed | Automatic recovery is bounded | Inspect concrete cause | Retry after correction | No endless retries |

## Error response contract

Machine responses should include an error code, a specific message, a concrete next action, and whether retry is safe. Human output should lead with the resource or host affected and avoid printing secrets or complete payloads.

Examples of intended wording:

- “This issue is still assigned to thread A on your local host. That host is offline. Reconnect it; RightWake has not moved the work.”
- “Two existing threads are candidates. A owns this issue; B only references it. Confirm the intended owner once to reuse that choice for later events.”
- “The draft changed after review. The previous approval is invalid for revision 3. Review the current text and destination before sending.”
- “The provider request timed out after dispatch. The reply may already exist. Reconcile this send before trying again.”
- “This Codex connection cannot guarantee exclusive writing of the selected thread. The event remains queued. Establish supported ownership before dispatch.”

## What counts as success

Installation success identifies a runnable binary and version. Plugin success identifies a loaded plugin and its local tools. Service success identifies a running managed receiver. Event success identifies durable receipt. Execution success identifies the actual existing thread, accepted turn, and terminal completion. Publication success identifies the resulting provider reply. Never collapse these into a single green “connected” status.


## Inspecting an interrupted router

`right-wake --json router dispatches` shows durable router attempts separately from work jobs. `router reconcile EVENT_ID` reads a bounded page of the original Codex router thread. An absent result in that page does not prove the turn never started.

After establishing concrete evidence on the host, an operator may use `--not-started-evidence TEXT` to release an unknown attempt for a fresh routing attempt, or `--completed-evidence TEXT` to record completion and leave ownership for explicit assignment. Neither option creates a thread or automatically accepts a semantic candidate. These recovery mutations are not exposed through MCP.

`drafts skip` applies only to a draft or stale draft; sending, sent, and uncertain publications cannot be hidden by skipping. `backup --output PATH` creates a private SQLite snapshot and refuses every existing destination, including symbolic links. Use a new path on a filesystem that supports hard links.

## Pause one host

Use `right-wake pause --host HOST_ID` to retain its bindings and hold future work without pausing other hosts. `right-wake resume --host HOST_ID` wakes that same host's known-safe waiting work; it also works before a later manual `run-once`. The host flag cannot be combined with source/resource scope. Resuming a host does not remove global/source pauses or replay an unknown dispatch. Pause stops work that has not been submitted; it does not withdraw an already accepted model turn.
