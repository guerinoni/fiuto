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

    /// Print the raw per-request results as JSON before the summary
    #[clap(long)]
    json: bool,

    /// Milliseconds to wait between requests to avoid hitting rate limits (429)
    #[clap(long, default_value_t = 0)]
    delay: u64,

    /// Apply the delay only after every N requests instead of after each one
    #[clap(long = "delay-every", default_value_t = 1)]
    delay_every: usize,
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

    let openapi_schema = match fiuto::parse_openapi(&s) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Cannot parse OpenAPI document: {e}");
            std::process::exit(1);
        }
    };

    let throttle = fiuto::Throttle {
        delay: std::time::Duration::from_millis(args.delay),
        every: args.delay_every.max(1),
    };

    let all_results = match fiuto::do_it_with_throttle(openapi_schema, args.base_url, args.jwt, throttle).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Error executing operations: {:?}", e);
            std::process::exit(1);
        }
    };

    if args.json {
        for r in &all_results {
            let string_results = serde_json::to_string_pretty(&r).unwrap(); // FIXME: handle the error
            println!("{string_results}");
        }
    }

    print_summary(&all_results);
}

/// Renders a fixed-width bar scaled so that `max` fills `width` cells.
fn bar(value: u32, max: u32, width: usize) -> String {
    if max == 0 {
        return String::new();
    }

    let filled = (value as usize * width) / max as usize;
    // Always show at least one cell for a non-zero value so small counts
    // don't render as an empty bar.
    let filled = filled.max(usize::from(value > 0));

    "█".repeat(filled)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }

    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

fn print_summary(all_results: &[Vec<fiuto::CallResult>]) {
    let endpoints = all_results.len();

    let mut codes: std::collections::BTreeMap<u16, u32> = std::collections::BTreeMap::new();
    let mut classes = [0u32; 5]; // index 0 -> 1xx, ... index 4 -> 5xx
    let mut total = 0u32;

    for r in all_results {
        for cr in r {
            *codes.entry(cr.status_code).or_default() += 1;
            total += 1;

            let class = (cr.status_code / 100) as usize;
            if (1..=5).contains(&class) {
                classes[class - 1] += 1;
            }
        }
    }

    println!();
    println!("════════════════════ fiuto summary ════════════════════");
    println!("requests: {total}    endpoints: {endpoints}");

    let class_labels = [
        "1xx info",
        "2xx ok",
        "3xx redirect",
        "4xx client",
        "5xx server",
    ];
    let max_class = classes.iter().copied().max().unwrap_or(0);

    println!();
    println!("by class");
    for (i, count) in classes.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        println!(
            "  {:<13} {:>4}  {}",
            class_labels[i],
            count,
            bar(*count, max_class, 30)
        );
    }

    let max_code = codes.values().copied().max().unwrap_or(0);

    println!();
    println!("by status code");
    for (code, count) in &codes {
        println!("  {code:>3} {count:>4}  {}", bar(*count, max_code, 30));
    }

    // A fuzzer driving random payloads should never make the server crash,
    // so surface every 5xx as a likely bug with the payload that caused it.
    let server_errors: Vec<&fiuto::CallResult> = all_results
        .iter()
        .flatten()
        .filter(|cr| cr.status_code >= 500)
        .collect();

    if !server_errors.is_empty() {
        println!();
        println!(
            "⚠ {} server error(s) (5xx), possible bugs",
            server_errors.len()
        );
        for cr in server_errors.iter().take(20) {
            let payload = if cr.payload.is_empty() {
                "<empty>".to_owned()
            } else {
                truncate(&cr.payload, 80)
            };
            println!("  {} {}  {}", cr.status_code, cr.path, payload);
        }
        if server_errors.len() > 20 {
            println!("  ... {} more", server_errors.len() - 20);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bar, truncate};

    #[test]
    fn bar_is_empty_when_max_is_zero() {
        // Guard against divide-by-zero when no requests were recorded.
        assert_eq!(bar(0, 0, 30), "");
        assert_eq!(bar(5, 0, 30), "");
    }

    #[test]
    fn bar_fills_full_width_at_max() {
        assert_eq!(bar(10, 10, 30), "█".repeat(30));
    }

    #[test]
    fn bar_scales_proportionally() {
        // 1/10 of 30 cells -> 3 cells.
        assert_eq!(bar(1, 10, 30), "█".repeat(3));
    }

    #[test]
    fn bar_shows_at_least_one_cell_for_nonzero() {
        // 1/100 rounds to 0 cells but a non-zero count must stay visible.
        assert_eq!(bar(1, 100, 30), "█");
    }

    #[test]
    fn bar_zero_value_renders_empty() {
        assert_eq!(bar(0, 10, 30), "");
    }

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_keeps_string_at_exact_limit() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_cuts_long_strings_and_appends_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[test]
    fn truncate_counts_chars_not_bytes() {
        // Multibyte chars must not be split mid-codepoint.
        assert_eq!(truncate("àéîõü", 5), "àéîõü");
        assert_eq!(truncate("àéîõü", 3), "àéî…");
    }
}
