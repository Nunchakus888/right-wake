use crate::{
    config::{Config, HostConfig, RouterConfig},
    db::Db,
    engine::Engine,
    model::*,
    outbound::{self, ApprovalEnvelope, ApprovedAction, CurrentDraftState, SendOutcome},
    provider::{self, IngestOutcome, SourceConfig},
};
use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(
    name = "waking",
    version,
    about = "Wake the right thread. Event-driven continuity for local agents."
)]
pub struct Cli {
    #[arg(long, global = true, env = "WAKING_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
    /// Versioned JSON on stdout. Diagnostics are always on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    /// Read plain NAME=VALUE secrets; never evaluate shell syntax.
    #[arg(long, global = true)]
    pub env_file: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create private local state idempotently; no service or source is enabled implicitly.
    Setup {
        #[arg(long)]
        listen: Option<String>,
    },
    /// Machine-readable commands, configuration, and MCP tool contracts.
    Schema,
    /// Show receipt, queue, completion, and publication as separate states.
    Status,
    /// Check local configuration and optionally probe Codex without a model turn.
    Doctor {
        #[arg(long)]
        probe: bool,
    },
    /// Receive signed webhooks. Foreground; no scheduled model calls.
    Serve {
        #[arg(long)]
        listen: Option<String>,
        #[arg(long)]
        relay_only: bool,
    },
    /// Run the stdio MCP management interface. Does not start the daemon.
    Mcp,
    #[command(subcommand)]
    Source(SourceCommand),
    #[command(subcommand)]
    Host(HostCommand),
    /// Assign a resource or discussion to one exact existing thread.
    Bind(BindArgs),
    Bindings,
    #[command(subcommand)]
    Context(ContextCommand),
    #[command(subcommand)]
    Router(RouterCommand),
    #[command(subcommand)]
    Events(EventCommand),
    #[command(subcommand)]
    Jobs(JobCommand),
    #[command(subcommand)]
    Drafts(DraftCommand),
    #[command(subcommand)]
    Service(ServiceCommand),
    #[command(subcommand)]
    Relay(RelayCommand),
    /// Pause automatic work globally, for a host/source, or one binding; retain input.
    Pause {
        #[arg(long, conflicts_with = "host")]
        source: Option<String>,
        #[arg(long, requires = "source", conflicts_with = "host")]
        resource: Option<String>,
        #[arg(long, conflicts_with_all = ["source", "resource"])]
        host: Option<String>,
    },
    /// Resume the selected scope; host resume explicitly wakes its durable waiters.
    Resume {
        #[arg(long, conflicts_with = "host")]
        source: Option<String>,
        #[arg(long, requires = "source", conflicts_with = "host")]
        resource: Option<String>,
        #[arg(long, conflicts_with_all = ["source", "resource"])]
        host: Option<String>,
    },
    /// Submit a signed local fixture through the same verification and durable inbox.
    Ingest {
        #[arg(long)]
        source: String,
        #[arg(long)]
        body: PathBuf,
        #[arg(long)]
        headers: PathBuf,
    },
    /// Perform a finite event pass. Refuses while a daemon owns this state.
    RunOnce,
    Backup {
        #[arg(long)]
        output: PathBuf,
    },
    /// Run a credential-free signed-event demo using two explicit mock threads.
    Demo,
    #[command(hide = true)]
    DemoWorker,
}
#[derive(Debug, Clone, ValueEnum)]
pub enum Kind {
    Linear,
    Github,
    Gitlab,
    Generic,
}
impl From<Kind> for ProviderKind {
    fn from(k: Kind) -> Self {
        match k {
            Kind::Linear => Self::Linear,
            Kind::Github => Self::Github,
            Kind::Gitlab => Self::Gitlab,
            Kind::Generic => Self::Generic,
        }
    }
}
#[derive(Debug, Subcommand)]
pub enum SourceCommand {
    /// Add from flags, or --file for the complete source configuration.
    Add(Box<SourceAddArgs>),
    List,
}
#[derive(Debug, Args)]
pub struct SourceAddArgs {
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    id: Option<String>,
    #[arg(long, value_enum)]
    kind: Option<Kind>,
    #[arg(long)]
    secret_env: Option<String>,
    #[arg(long)]
    token_env: Option<String>,
    #[arg(long)]
    tenant: Option<String>,
    #[arg(long)]
    instance: Option<String>,
    #[arg(long)]
    trigger: Vec<String>,
    #[arg(long)]
    resource: Vec<String>,
    #[arg(long)]
    bot_actor: Vec<String>,
}
#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// A socket selects Codex; a program selects a trusted stdin JSON adapter.
    Add {
        #[arg(long)]
        id: String,
        #[arg(long, conflicts_with = "program")]
        socket: Option<PathBuf>,
        #[arg(long, conflicts_with = "socket")]
        program: Option<PathBuf>,
        #[arg(long)]
        arg: Vec<String>,
        /// Fixed working directory for a trusted command adapter.
        #[arg(long, requires = "program")]
        cwd: Option<PathBuf>,
        #[arg(long)]
        exclusive_writer: bool,
        #[arg(long)]
        restricted_tools: bool,
        #[arg(long, default_value_t = 600)]
        timeout: u64,
    },
    List,
    Threads {
        #[arg(long)]
        host: String,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        cursor: Option<String>,
    },
}
#[derive(Debug, Args)]
pub struct BindArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub resource: String,
    #[arg(long)]
    pub host: String,
    #[arg(long)]
    pub thread: String,
    #[arg(long)]
    pub cwd: Option<String>,
    /// Optimistic concurrency check. 0 means the binding must not yet exist.
    #[arg(long)]
    pub expected_version: Option<i64>,
}
#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    Put {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expected_revision: Option<i64>,
    },
    List {
        #[arg(long)]
        source: Option<String>,
    },
    Search {
        #[arg(long)]
        source: String,
        #[arg(long)]
        query: String,
    },
    /// Read one host page, registering metadata only. No summarization model.
    Index {
        #[arg(long)]
        host: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Read bounded real history for a selected existing card, without executing it.
    Refresh {
        #[arg(long)]
        id: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum RouterCommand {
    Set {
        #[arg(long)]
        host: String,
        #[arg(long)]
        thread: String,
        #[arg(long, required = true)]
        source: Vec<String>,
        #[arg(long, default_value_t = 5)]
        max_candidates: usize,
    },
    Show,
    Clear,
    /// Inspect persistent router dispatches, including unknown outcomes.
    Dispatches,
    /// Resolve an unknown dispatch only with concrete evidence from its host.
    Reconcile {
        event_id: String,
        #[arg(long, conflicts_with = "completed_evidence")]
        not_started_evidence: Option<String>,
        #[arg(long, conflicts_with = "not_started_evidence")]
        completed_evidence: Option<String>,
    },
}
#[derive(Debug, Subcommand)]
pub enum EventCommand {
    List {
        #[arg(long)]
        state: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Show {
        id: String,
    },
}
#[derive(Debug, Subcommand)]
pub enum JobCommand {
    List {
        #[arg(long)]
        state: Option<String>,
    },
    Show {
        id: String,
    },
    Retry {
        id: String,
    },
    Reconcile {
        id: String,
        /// Required evidence from the host that it never accepted this dispatch.
        #[arg(long)]
        not_started_evidence: Option<String>,
    },
}
#[derive(Debug, Subcommand)]
pub enum DraftCommand {
    List,
    Show {
        id: String,
    },
    /// Export exact review bytes; this does not approve or publish anything.
    Export {
        id: String,
        #[arg(long, default_value_t = 3600)]
        expires_in: u64,
    },
    Edit {
        id: String,
        #[arg(long)]
        body: PathBuf,
    },
    Skip {
        id: String,
    },
    /// Send only after an external reviewer signed the exact exported statement.
    Send {
        id: String,
        #[arg(long)]
        approval: PathBuf,
    },
}
#[derive(Debug, Subcommand)]
pub enum ServiceCommand {
    Install,
    Start,
    Stop,
    Status,
    Uninstall,
}
#[derive(Debug, Subcommand)]
pub enum RelayCommand {
    Serve {
        #[arg(long)]
        listen: Option<String>,
    },
    /// Pair a device with exact source scopes; put the secret in an environment variable.
    Device {
        #[arg(long)]
        id: String,
        #[arg(long)]
        token_env: String,
        #[arg(long, required = true)]
        source: Vec<String>,
        #[arg(long)]
        revoke: bool,
    },
    Connect {
        #[arg(long)]
        url: String,
        #[arg(long)]
        token_env: String,
        #[arg(long, required = true)]
        source: Vec<String>,
        #[arg(long)]
        allow_loopback: bool,
    },
}

pub fn read_json(path: &Path) -> Result<Value> {
    let bytes = read_bounded(path, 4 * 1024 * 1024)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    if path == Path::new("-") {
        std::io::stdin()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
    } else {
        std::fs::File::open(path)?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
    }
    if bytes.len() > limit {
        bail!("input too large");
    }
    Ok(bytes)
}
pub fn load_env_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
            bail!("environment file must be private (chmod 600)");
        }
    }
    let text = String::from_utf8(read_bounded(path, 65536)?)?;
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .with_context(|| format!("env file line {} must be NAME=VALUE", line_no + 1))?;
        crate::config::validate_env(k)?;
        if matches!(
            k,
            "PATH"
                | "HOME"
                | "CODEX_HOME"
                | "LD_PRELOAD"
                | "LD_LIBRARY_PATH"
                | "DYLD_INSERT_LIBRARIES"
                | "DYLD_LIBRARY_PATH"
        ) {
            bail!("environment file may only provide credentials, not process execution settings");
        }
        std::env::set_var(k, v);
    }
    Ok(())
}

pub async fn execute(cli: Cli) -> Result<Option<Value>> {
    let dir = cli.data_dir.unwrap_or_else(crate::config::default_data_dir);
    if let Some(path) = &cli.env_file {
        load_env_file(path)?;
    }
    match cli.command {
        Command::Schema => return Ok(Some(crate::mcp::schema())),
        Command::Setup { listen } => {
            let mut cfg = Config::setup(&dir)?;
            if let Some(listen) = listen {
                cfg.listen = listen;
                cfg.save(&dir)?;
            }
            Db::open(&dir)?;
            return Ok(Some(
                json!({"state":"configured","data_dir":dir,"next_actions":["waking source add --help","waking host add --help","waking bind --help","waking serve"],"service_started":false,"webhook_verified":false}),
            ));
        }
        Command::Demo => return Ok(Some(crate::demo::run(&dir).await?)),
        Command::DemoWorker => {
            crate::demo::worker(&dir)?;
            return Ok(None);
        }
        Command::Mcp => {
            crate::mcp::run(dir).await?;
            return Ok(None);
        }
        _ => {}
    }
    let mut cfg = Config::load(&dir)?;
    let db = Db::open(&dir)?;
    let engine = Engine::new(dir.clone(), db.clone());
    let value=match cli.command {
        Command::Status=>{let mut status=db.status()?;status["paused"]=json!(cfg.paused);status["paused_hosts"]=json!(cfg.paused_hosts);status["daemon_running"]=json!(daemon_running(&dir));status["next_action"]=json!("Inspect events/jobs for the exact waiting reason. Received, completed, and sent are separate states.");status}
        Command::Doctor{probe}=>doctor(&engine,&cfg,probe).await?,
        Command::Serve{listen,relay_only}=>{crate::server::serve(engine,listen,relay_only).await?;return Ok(None);}
        Command::Source(SourceCommand::List)=>serde_json::to_value(&cfg.sources)?,
        Command::Source(SourceCommand::Add(args))=>{
            let SourceAddArgs{file,id,kind,secret_env,token_env,tenant,instance,trigger,resource,bot_actor}=*args;
            let source:SourceConfig=if let Some(file)=file{serde_json::from_value(read_json(&file)?)?}else{
                serde_json::from_value(json!({"id":id.context("--id is required")?,"kind":ProviderKind::from(kind.context("--kind is required")?),"secret_env":secret_env.context("--secret-env is required")?,"token_env":token_env,"tenant_id":tenant,"instance":instance.unwrap_or_default(),"trigger_patterns":if trigger.is_empty(){vec!["@waking".to_owned()]}else{trigger},"allowed_resources":resource,"bot_actor_ids":bot_actor}))?
            };
            source.validate()?;
            if let Some(old)=cfg.sources.iter_mut().find(|s|s.id==source.id){*old=source.clone();}else{cfg.sources.push(source.clone());}
            cfg.save(&dir)?;crate::server::poke(&dir);json!({"source_id":source.id,"state":"configured","webhook_path":format!("/hooks/{}",source.id),"next_action":"Set the named secret environment variable, configure the provider webhook, then run serve"})
        }
        Command::Host(HostCommand::List)=>serde_json::to_value(&cfg.hosts)?,
        Command::Host(HostCommand::Add{id,socket,program,arg,cwd,exclusive_writer,restricted_tools,timeout})=>{
            let host=if let Some(socket)=socket{HostConfig::Codex{id:id.clone(),socket,exclusive_writer,restricted_tools,timeout_seconds:timeout}}else if let Some(program)=program{HostConfig::Command{id:id.clone(),program,args:arg,cwd,timeout_seconds:timeout}}else{bail!("provide --socket for Codex or --program for a command adapter")};
            if let Some(old)=cfg.hosts.iter_mut().find(|h|h.id()==id){*old=host;}else{cfg.hosts.push(host);}
            cfg.save(&dir)?;crate::server::poke(&dir);json!({"host_id":id,"state":"configured","next_action":"Run doctor --probe; bind exact existing thread UUIDs"})
        }
        Command::Host(HostCommand::Threads{host,archived,cursor})=>engine.client(cfg.host(&host)?).await?.list_threads(json!({"limit":50,"cursor":cursor,"archived":archived,"modelProviders":[],"sourceKinds":["cli","vscode","exec","appServer","unknown"]})).await?,
        Command::Bind(args)=>{
            cfg.source(&args.source)?;cfg.host(&args.host)?;
            let target=ThreadTarget{host_id:args.host,thread_id:args.thread,cwd:args.cwd};
            if cfg.router.as_ref().is_some_and(|r|r.target.host_id==target.host_id&&r.target.thread_id==target.thread_id){bail!("router thread cannot own ordinary work");}
            if matches!(cfg.host(&target.host_id)?,HostConfig::Codex{..}){uuid::Uuid::parse_str(&target.thread_id).context("Codex binding needs the exact thread UUID, not a title or family session id")?;}
            let b=db.bind(&args.source,&args.resource,&target,"explicit",args.expected_version)?;
            crate::server::poke(&dir);serde_json::to_value(b)?
        }
        Command::Bindings=>serde_json::to_value(db.bindings()?)?,
        Command::Context(ContextCommand::List{source})=>serde_json::to_value(db.cards(source.as_deref())?)?,
        Command::Context(ContextCommand::Put{file,expected_revision})=>{let card:ContextCard=serde_json::from_value(read_json(&file)?)?;cfg.host(&card.target.host_id)?;for source in &card.source_ids{cfg.source(source)?;}
            let old=db.card(&card.id).ok();let trusted:Vec<_>=card.evidence.iter().filter(|e|e.starts_with("ownership:")).collect();
            if let Some(old)=old {let expected:Vec<_>=old.evidence.iter().filter(|e|e.starts_with("ownership:")).collect();if trusted!=expected||card.owned_resources!=old.owned_resources||(!expected.is_empty()&&(card.target!=old.target||card.source_ids!=old.source_ids)){bail!("ownership fields can only change through an explicit bind, not a context note");}}
            else if !trusted.is_empty()||!card.owned_resources.is_empty(){bail!("new context cards cannot claim ownership; use bind after review");}
            serde_json::to_value(db.put_card(card,expected_revision)?)?}
        Command::Context(ContextCommand::Search{source,query})=>{cfg.source(&source)?;let q=query.to_lowercase();serde_json::to_value(db.cards(Some(&source))?.into_iter().filter(|c|format!("{} {}",c.title,c.summary).to_lowercase().contains(&q)).take(10).collect::<Vec<_>>())?}
        Command::Context(ContextCommand::Index{host,source,cursor})=>{
            cfg.source(&source)?;let page=engine.client(cfg.host(&host)?).await?.list_threads(json!({"limit":50,"cursor":cursor,"archived":false,"modelProviders":[],"sourceKinds":["cli","vscode","exec","appServer","unknown"]})).await?;
            let mut registered=0;for thread in page["data"].as_array().into_iter().flatten(){let Some(id)=thread["id"].as_str()else{continue};if cfg.router.as_ref().is_some_and(|r|r.target.host_id==host&&r.target.thread_id==id){continue;}let key=format!("{host}:{id}");if db.card(&key).is_ok(){continue;}db.put_card(ContextCard{id:key,target:ThreadTarget{host_id:host.clone(),thread_id:id.into(),cwd:thread["cwd"].as_str().map(str::to_owned)},source_ids:vec![source.clone()],title:thread["name"].as_str().or(thread["preview"].as_str()).unwrap_or("Untitled thread").chars().take(300).collect(),summary:String::new(),owned_resources:vec![],referenced_resources:vec![],evidence:vec![],open_questions:vec![],revision:0,coverage:"discovered_metadata_only".into(),enabled:true},Some(0))?;registered+=1;}
            json!({"registered":registered,"next_cursor":page["nextCursor"],"coverage":"one_page_metadata_only","next_action":"Refresh relevant cards or provide reviewed summaries before semantic routing"})
        }
        Command::Context(ContextCommand::Refresh{id})=>{let mut card=db.card(&id)?;let page=engine.client(cfg.host(&card.target.host_id)?).await?.list_turns(&card.target,None,5).await?;let excerpt=serde_json::to_string(&page)?;card.summary=excerpt.chars().take(12000).collect();card.coverage="partial:last_5_turns".into();card.evidence.push(format!("history_sha256:{}",digest(excerpt.as_bytes())));let revision=card.revision;serde_json::to_value(db.put_card(card,Some(revision))?)?}
        Command::Router(RouterCommand::Set{host,thread,source,max_candidates})=>{
            cfg.host(&host)?;let target=ThreadTarget{host_id:host,thread_id:thread,cwd:None};
            if db.bindings()?.iter().any(|b|b.target.host_id==target.host_id&&b.target.thread_id==target.thread_id){bail!("router needs a dedicated thread with no work bindings");}
            if let Some(old)=&cfg.router {if old.target.host_id==target.host_id && old.target.thread_id==target.thread_id && old.source_ids!=source{bail!("a router thread cannot change trust scope; choose a fresh dedicated router thread");}}
            cfg.router=Some(RouterConfig{target,source_ids:source,max_candidates});cfg.save(&dir)?;json!({"state":"configured","policy":"Event-triggered, scoped candidate recommendations; exact ownership or explicit claim establishes binding"})
        }
        Command::Router(RouterCommand::Show)=>serde_json::to_value(&cfg.router)?,
        Command::Router(RouterCommand::Clear)=>{cfg.router=None;cfg.save(&dir)?;json!({"state":"disabled"})}
        Command::Router(RouterCommand::Dispatches)=>json!(db.router_dispatches()?),
        Command::Router(RouterCommand::Reconcile{event_id,not_started_evidence,completed_evidence})=>{
            if let Some(evidence)=not_started_evidence {
                db.reconcile_router_not_started(&event_id,&evidence)?;
                crate::server::poke(&dir);
                json!({"event_id":event_id,"state":"received","next_action":"The host-confirmed unstarted event may be routed again"})
            } else if let Some(evidence)=completed_evidence {
                db.reconcile_router_completed(&event_id,&evidence)?;
                crate::server::poke(&dir);
                json!({"event_id":event_id,"state":"unassigned","next_action":"Review the completed router result and explicitly bind the owner; no automatic rerun"})
            } else {
                let dispatch=db.router_dispatches()?.into_iter().find(|row|row["event_id"].as_str()==Some(&event_id)).context("router dispatch not found")?;
                let target:ThreadTarget=serde_json::from_value(dispatch["target"].clone())?;
                let history=engine.client(cfg.host(&target.host_id)?).await?.list_turns(&target,None,20).await?;
                json!({"dispatch":dispatch,"history":history,"next_action":"Match dispatch_id and the host's final outcome. Absence in this bounded page is not proof that execution never started."})
            }
        }
        Command::Events(EventCommand::List{state,limit})=>json!(db.events(state.as_deref(),limit)?),
        Command::Events(EventCommand::Show{id})=>db.event_detail(&id)?,
        Command::Jobs(JobCommand::List{state})=>serde_json::to_value(db.jobs(state.as_deref(),1000)?)?,
        Command::Jobs(JobCommand::Show{id})=>serde_json::to_value(db.job(&id)?)?,
        Command::Jobs(JobCommand::Retry{id})=>{db.retry(&id)?;crate::server::poke(&dir);json!({"job_id":id,"state":"queued"})}
        Command::Jobs(JobCommand::Reconcile{id,not_started_evidence})=>{
            if let Some(evidence)=not_started_evidence{db.reconcile_not_started(&id,&evidence)?;json!({"job_id":id,"state":"rejected","next_action":"Retry only after the recorded host evidence is verified"})}else{let job=db.job(&id)?;let history=engine.client(cfg.host(&job.target.host_id)?).await?.list_turns(&job.target,None,20).await?;json!({"job":job,"history":history,"next_action":"Match the exact dispatch_id and turn outcome. Absence in this bounded page does not prove it never started; do not resend unknown work."})}
        }
        Command::Drafts(DraftCommand::List)=>serde_json::to_value(db.drafts()?)?,
        Command::Drafts(DraftCommand::Show{id})=>serde_json::to_value(db.draft(&id)?)?,
        Command::Drafts(DraftCommand::Export{id,expires_in})=>{
            if expires_in==0||expires_in>86400{bail!("approval expiry must be within 1..86400 seconds");}let draft=db.draft(&id)?;let source=cfg.source(&draft.source_id)?;let statement=outbound::prepare_approval_statement(source,&draft,now_ms()+(expires_in as i64)*1000)?;use base64::Engine as _;json!({"draft":draft,"statement":statement,"review_target_json":String::from_utf8(outbound::approval_target_bytes(source,&draft)?)?,"signing_bytes_base64":base64::engine::general_purpose::STANDARD.encode(outbound::approval_signing_bytes(&statement)?),"approved":false,"next_action":"Review exact text, target and identity on a separate reviewer device; return a signed ApprovalEnvelope"})
        }
        Command::Drafts(DraftCommand::Edit{id,body})=>{let body=String::from_utf8(read_bounded(&body,65536)?)?;serde_json::to_value(db.edit_draft(&id,&body)?)?}
        Command::Drafts(DraftCommand::Skip{id})=>{db.skip_draft(&id)?;json!({"draft_id":id,"state":"skipped"})}
        Command::Drafts(DraftCommand::Send{id,approval})=>{
            let draft=db.draft(&id)?;let source=cfg.source(&draft.source_id)?;let binding=db.binding_for(&db.event(&draft.event_id)?)?.context("binding missing")?;if binding.key!=draft.binding_key{bail!("draft_stale: a more specific binding changed the owner");}
            let current=CurrentDraftState{source_revision:db.resource_revision(&draft.source_id,&draft.resource_key)?,binding_version:binding.version,binding_enabled:binding.enabled,draft:draft.clone()};
            let approval:ApprovalEnvelope=serde_json::from_value(read_json(&approval)?)?;let action=ApprovedAction{draft,approval};outbound::verify_approval(source,&action,&current,now_ms())?;
            db.begin_send(&id)?;
            let result=outbound::send_approved_guarded(source,&action,&current,now_ms(),||{
                let latest_cfg=Config::load(&dir)?;let latest_source=latest_cfg.source(&action.draft.source_id)?.clone();let latest=db.draft(&id)?;let binding=db.binding_for(&db.event(&latest.event_id)?)?.context("binding missing")?;if binding.key!=latest.binding_key{bail!("draft_stale: effective binding changed");}
                Ok((latest_source,CurrentDraftState{source_revision:db.resource_revision(&latest.source_id,&latest.resource_key)?,binding_version:binding.version,binding_enabled:binding.enabled,draft:latest}))
            }).await;
            let (state,reason)=match &result{SendOutcome::Sent{..}=>("sent","Provider confirmed the write"),SendOutcome::Unknown{reason}=>("unknown",reason.as_str()),SendOutcome::Rejected{reason}=>("draft",reason.as_str()),SendOutcome::Failed{reason,..}=>("draft",reason.as_str())};
            db.draft_state(&id,state,reason,&serde_json::to_value(&result)?)?;serde_json::to_value(result)?
        }
        Command::Service(action)=>{let action=match action{ServiceCommand::Install=>"install",ServiceCommand::Start=>"start",ServiceCommand::Stop=>"stop",ServiceCommand::Status=>"status",ServiceCommand::Uninstall=>"uninstall"};crate::service::manage(action,&dir,&std::env::current_exe()?,cli.env_file.as_deref())?}
        Command::Relay(RelayCommand::Serve{listen})=>{crate::server::serve(engine,listen,true).await?;return Ok(None);}
        Command::Relay(RelayCommand::Device{id,token_env,source,revoke})=>{crate::config::validate_id(&id)?;crate::config::validate_env(&token_env)?;for s in &source{cfg.source(s)?;}let device=crate::config::RelayDevice{id:id.clone(),token_env,source_ids:source,enabled:!revoke};if let Some(old)=cfg.relay_devices.iter_mut().find(|d|d.id==id){*old=device;}else{cfg.relay_devices.push(device);}cfg.save(&dir)?;crate::server::poke(&dir);json!({"device_id":id,"enabled":!revoke})}
        Command::Relay(RelayCommand::Connect{url,token_env,source,allow_loopback})=>{crate::relay::connect(engine,&url,&token_env,&source,allow_loopback).await?;return Ok(None);}
        Command::Pause{source,resource,host}=>set_paused(&dir,&db,&mut cfg,source,resource,host,true)?,
        Command::Resume{source,resource,host}=>set_paused(&dir,&db,&mut cfg,source,resource,host,false)?,
        Command::Ingest{source,body,headers}=>{let raw=read_bounded(&body,cfg.max_payload_bytes)?;let headers=serde_json::from_value(read_json(&headers)?)?;match provider::ingest(cfg.source(&source)?,&headers,&raw,now_ms())?{IngestOutcome::Event(e)=>{let receipt=db.receive(&e,&raw,&cfg)?;crate::server::poke(&dir);receipt},IngestOutcome::Ignored{reason}=>json!({"state":"ignored","reason":reason})}}
        Command::RunOnce=>{let _lock=acquire_worker_lock(&dir)?;let recovered=db.recover()?;json!({"processed":engine.run_once().await?,"recovered_unknown":recovered})}
        Command::Backup{output}=>{db.backup(&output)?;json!({"backup":output})}
        _=>unreachable!(),
    };
    Ok(Some(value))
}

pub fn acquire_worker_lock(dir: &Path) -> Result<std::fs::File> {
    use fs2::FileExt;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("daemon.lock"))?;
    file.try_lock_exclusive().context(
        "daemon_already_running: use the resident worker instead of a second dispatcher",
    )?;
    Ok(file)
}
fn daemon_running(dir: &Path) -> bool {
    acquire_worker_lock(dir).is_err()
}
fn set_paused(
    dir: &Path,
    db: &Db,
    cfg: &mut Config,
    source: Option<String>,
    resource: Option<String>,
    host: Option<String>,
    paused: bool,
) -> Result<Value> {
    if let Some(host) = host {
        if source.is_some() || resource.is_some() {
            bail!("--host conflicts with --source and --resource");
        }
        cfg.host(&host)?;
        if paused {
            if !cfg.host_paused(&host) {
                cfg.paused_hosts.push(host.clone());
            }
        } else {
            cfg.paused_hosts.retain(|id| id != &host);
        }
        cfg.save(dir)?;
        if !paused {
            // Required even without a resident worker: run-once does not
            // recover held work, and a filesystem notification alone is lost.
            db.wake_waiters(Some(&host))?;
        }
        crate::server::poke(dir);
        return Ok(
            json!({"paused":paused,"host_id":host,"paused_hosts":cfg.paused_hosts,"data_retained":true}),
        );
    }
    match (source, resource) {
        (Some(source), Some(key)) => db.pause_binding(&source, &key, paused)?,
        (Some(source), None) => {
            cfg.sources
                .iter_mut()
                .find(|s| s.id == source)
                .context("source not found")?
                .enabled = !paused;
            cfg.save(dir)?;
        }
        (None, None) => {
            cfg.paused = paused;
            cfg.save(dir)?;
        }
        _ => bail!("--resource requires --source"),
    };
    crate::server::poke(dir);
    Ok(json!({"paused":paused,"data_retained":true}))
}
async fn doctor(engine: &Engine, cfg: &Config, probe: bool) -> Result<Value> {
    let mut sources = vec![];
    for source in &cfg.sources {
        sources.push(json!({"id":source.id,"enabled":source.enabled,"secret_available":std::env::var(&source.secret_env).is_ok_and(|s|!s.is_empty()),"public_webhook_verified":false,"next_action":"Register the public HTTPS webhook and deliver a signed test event"}));
    }
    let mut hosts = vec![];
    for host in &cfg.hosts {
        let mut result = match host {
            HostConfig::Codex {
                exclusive_writer,
                restricted_tools,
                socket,
                ..
            } => {
                let connected = if probe {
                    Some(engine.client(host).await.is_ok())
                } else {
                    None
                };
                json!({"id":host.id(),"kind":"codex","socket":socket,"connected":connected,"exclusive_writer_declared":exclusive_writer,"restricted_tools_declared":restricted_tools,"thread_execution_verified":false,"next_action":if *exclusive_writer{"Probe a selected original thread on the actual host"}else{"Establish a single writer for automatic execution; keep shared Desktop threads held"}})
            }
            HostConfig::Command { program, .. } => {
                json!({"id":host.id(),"kind":"command","program_exists":program.is_file(),"sandboxed":false,"next_action":"Use a trusted adapter with the stdin correlation contract"})
            }
        };
        result["paused"] = json!(cfg.host_paused(host.id()));
        hosts.push(result);
    }
    Ok(
        json!({"version":env!("CARGO_PKG_VERSION"),"configuration":"valid","storage":engine.db.status()?,"daemon_running":daemon_running(&engine.dir),"sources":sources,"hosts":hosts,"model_turns_requested_by_doctor":0,"note":"Configuration and handshake checks do not prove model execution or external reply delivery"}),
    )
}
