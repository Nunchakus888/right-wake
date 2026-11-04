use clap::Parser;
use right_wake::{cli::Cli, model::ok};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let args: Vec<String> = std::env::args().collect();
    let machine = args.iter().any(|x| x == "--json");
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            if machine && e.use_stderr() {
                println!(
                    "{}",
                    serde_json::json!({"schema_version":1,"ok":false,"error":{"code":"invalid_arguments","message":e.to_string(),"next_action":"Run right-wake schema or the command's --help","retryable":false}})
                );
                std::process::exit(2);
            }
            e.exit();
        }
    };
    match right_wake::cli::execute(cli).await {
        Ok(Some(value)) => println!(
            "{}",
            if machine {
                ok(value).to_string()
            } else {
                serde_json::to_string_pretty(&ok(value)).expect("JSON value serializes")
            }
        ),
        Ok(None) => {}
        Err(error) => {
            let message = error.to_string();
            let code = message
                .split(':')
                .next()
                .filter(|s| s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
                .unwrap_or("operation_failed");
            let envelope = serde_json::json!({"schema_version":1,"ok":false,"error":{"code":code,"message":message,"next_action":"Run right-wake doctor; inspect the named event/job before retrying. Unknown outcomes require reconciliation.","retryable":false}});
            println!("{}", envelope);
            std::process::exit(1);
        }
    }
}
