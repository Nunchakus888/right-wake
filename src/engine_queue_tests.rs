//! Production queue transport and engine completion boundaries over in-memory
//! WebSockets. The fixture never starts Codex or calls a model/provider.
use super::*;
use crate::config::RouterConfig;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex as StdMutex};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn queue_completion_requires_real_context_tool_read_for_work_and_routing() {
    for routing in [false, true] {
        for read_context in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let target = ThreadTarget {
                host_id: "queue".into(),
                thread_id: "existing-thread".into(),
                cwd: None,
            };
            let host = HostConfig::Codex {
                id: "queue".into(),
                socket: dir.path().join("unused.sock"),
                delivery: CodexDelivery::Queue,
                exclusive_writer: false,
                restricted_tools: true,
                timeout_seconds: 3,
            };
            let config = Config {
                sources:vec![serde_json::from_value(json!({"id":"source","kind":"linear","tenant_id":"org","secret_env":"TEST_ONLY","sending_identity":"fixture"})).unwrap()],
                hosts:vec![host.clone()],
                router:routing.then(|| RouterConfig { target:target.clone(),source_ids:vec!["source".into()],max_candidates:5 }),
                ..Config::default()
            };
            config.save(dir.path()).unwrap();
            let db = Db::open(dir.path()).unwrap();
            let event = NormalizedEvent {
                event_id: "event".into(),
                delivery_id: "delivery".into(),
                source_id: "source".into(),
                provider: ProviderKind::Linear,
                tenant_id: "org".into(),
                resource_key: "linear:https://linear.app:org:issue".into(),
                conversation_key: None,
                actor_id: "person".into(),
                action: "comment.create".into(),
                occurred_at_ms: 1,
                received_at_ms: 1,
                source_url: None,
                content: "UNTRUSTED_EXTERNAL_PAYLOAD should remain in tool output".into(),
                revision: "source-body".into(),
                actionable: true,
                explicit_trigger: true,
                skip_reason: None,
                linear: None,
                reply_target: Some(ReplyTarget {
                    provider: ProviderKind::Linear,
                    container_id: "org".into(),
                    resource_id: "issue".into(),
                    comment_id: None,
                    discussion_id: None,
                }),
            };
            db.receive(&event, b"synthetic", &config).unwrap();
            if routing {
                db.put_card(
                    ContextCard {
                        id: "candidate".into(),
                        target: ThreadTarget {
                            host_id: "queue".into(),
                            thread_id: "candidate-thread".into(),
                            cwd: None,
                        },
                        source_ids: vec!["source".into()],
                        title: "Task".into(),
                        summary: "Candidate metadata".into(),
                        owned_resources: vec![],
                        referenced_resources: vec![],
                        evidence: vec![],
                        open_questions: vec![],
                        revision: 0,
                        coverage: "metadata_only".into(),
                        enabled: true,
                    },
                    Some(0),
                )
                .unwrap();
            } else {
                db.bind("source", &event.resource_key, &target, "explicit", Some(0))
                    .unwrap();
            }
            let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
            let calls = Arc::new(StdMutex::new(Vec::<Value>::new()));
            let recorded = calls.clone();
            let profile = dir.path().to_path_buf();
            let server = tokio::spawn(async move {
                let mut websocket = tokio_tungstenite::accept_async(server_stream)
                    .await
                    .unwrap();
                while let Some(Ok(Message::Text(text))) = websocket.next().await {
                    let call: Value = serde_json::from_str(&text).unwrap();
                    recorded.lock().unwrap().push(call.clone());
                    let method = call["method"].as_str().unwrap_or_default();
                    let result = match method {
                        "initialize" => json!({"userAgent":"engine-queue-fixture"}),
                        "initialized" => continue,
                        "thread/read" | "thread/resume" => {
                            json!({"thread":{"id":"existing-thread","ephemeral":false,"status":{"type":"idle"}}})
                        }
                        "thread/queue/add" => {
                            let dispatch_id =
                                call["params"]["clientUserMessageId"].as_str().unwrap();
                            assert!(!call["params"]["input"]
                                .to_string()
                                .contains("UNTRUSTED_EXTERNAL_PAYLOAD"));
                            if read_context {
                                let context = crate::mcp::call(
                                    profile.clone(),
                                    "right_wake_dispatch_context",
                                    &json!({"dispatch_id":dispatch_id}),
                                    &json!({"threadId":"existing-thread"}),
                                )
                                .await
                                .unwrap();
                                assert_eq!(context["payload"]["event"]["event_id"], "event");
                                assert_eq!(context["target"]["thread_id"], "existing-thread");
                                assert_eq!(context["output_schema"].is_object(), routing);
                            }
                            let input = call["params"]["input"].clone();
                            websocket.send(Message::Text(json!({"id":call["id"],"result":{"queuedSubmission":{"id":"queue-entry","clientUserMessageId":dispatch_id,"input":input}}}).to_string().into())).await.unwrap();
                            let output = if routing {
                                json!({"decision":"unassigned","candidate_id":null,"card_revision":null,"intent":"fixture","evidence":[],"reason":"no ownership inferred"})
                            } else {
                                json!({"reply":"A synthetic result","context_summary":"","open_questions":[]})
                            };
                            for notification in [
                                json!({"method":"item/started","params":{"threadId":"existing-thread","turnId":"turn","item":{"type":"userMessage","id":"user-item","clientId":dispatch_id,"content":input}}}),
                                json!({"method":"turn/completed","params":{"threadId":"existing-thread","turn":{"id":"turn","status":"completed","items":[{"type":"agentMessage","id":"answer","phase":"final_answer","text":output.to_string()}]}}}),
                            ] {
                                websocket
                                    .send(Message::Text(notification.to_string().into()))
                                    .await
                                    .unwrap();
                            }
                            continue;
                        }
                        other => panic!("unexpected queue fixture method: {other}"),
                    };
                    websocket
                        .send(Message::Text(
                            json!({"id":call["id"],"result":result}).to_string().into(),
                        ))
                        .await
                        .unwrap();
                }
            });
            let client = AppServerClient::connect_memory_for_test(
                AppServerOptions {
                    host_id: "queue".into(),
                    socket_path: dir.path().join("unused.sock"),
                    request_timeout_ms: 1000,
                    delivery: CodexDelivery::Queue,
                    exclusive_writer: false,
                },
                client_stream,
            )
            .await
            .unwrap();
            let engine = Engine::new(dir.path().into(), db.clone());
            engine.clients.lock().await.insert(
                "queue".into(),
                (serde_json::to_string(&host).unwrap(), Arc::new(client)),
            );
            engine.run_once().await.unwrap();
            if routing {
                let dispatches = db.router_dispatches().unwrap();
                assert_eq!(dispatches.len(), 1);
                assert_eq!(
                    dispatches[0]["state"],
                    if read_context { "completed" } else { "failed" }
                );
                if !read_context {
                    assert!(dispatches[0]["reason"]
                        .as_str()
                        .unwrap()
                        .starts_with("dispatch_context_not_read"));
                }
                assert!(db.drafts().unwrap().is_empty());
            } else {
                let jobs = db.jobs(None, 10).unwrap();
                assert_eq!(jobs.len(), 1);
                assert_eq!(
                    jobs[0].state,
                    if read_context { "drafted" } else { "failed" }
                );
                assert_eq!(db.drafts().unwrap().len(), usize::from(read_context));
                if !read_context {
                    assert!(jobs[0]
                        .reason
                        .as_deref()
                        .unwrap()
                        .starts_with("dispatch_context_not_read"));
                }
            }
            let calls = calls.lock().unwrap();
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call["method"] == "thread/queue/add")
                    .count(),
                1
            );
            assert!(calls.iter().all(|call| !matches!(
                call["method"].as_str(),
                Some("turn/start" | "thread/start" | "thread/fork" | "thread/queue/start")
            )));
            server.abort();
        }
    }
}
