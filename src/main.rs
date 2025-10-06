use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the openapi schema file
    openapi_file: String,

    /// Base URL to use for the requests
    #[clap(long, short)]
    base_url: Option<String>,

    /// Skip deprecated endpoints
    #[clap(long)]
    skip_deprecated: bool,

    /// Token JWT to use in request headers
    #[clap(long)]
    jwt: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let s = match std::fs::read_to_string(args.openapi_file) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Error reading file: {:?}", e);
            std::process::exit(1);
        }
    };

    let openapi_schema: openapiv3::OpenAPI = match serde_yaml_bw::from_str(&s) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Error parsing yaml: {:?}", e);
            std::process::exit(1);
        }
    };

    let all_results = match fiuto::do_it(openapi_schema, args.base_url, args.jwt).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Error executing operations: {:?}", e);
            std::process::exit(1);
        }
    };

    for r in &all_results {
        let string_results = serde_json::to_string_pretty(&r).unwrap(); // FIXME: handle the error
        println!("{string_results}");
    }

    let mut codes = std::collections::HashMap::new();

    for r in &all_results {
        for cr in r {
            let counter = codes.entry(cr.status_code).or_insert(0);
            let new_count = *counter + 1;
            codes.insert(cr.status_code, new_count);
        }
    }

    let table = tabled::Table::new(codes.iter().map(|(k, v)| StatsResult {
        status_code: *k,
        count: *v,
    }))
    .to_string();

    println!("{table}");
}

#[derive(tabled::Tabled)]
struct StatsResult {
    status_code: u16,
    count: u32,
}
