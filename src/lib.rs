mod collector;
mod digger;
mod parser;
mod shuffler;

pub use parser::parse_openapi;

#[derive(Debug, serde::Serialize)]
pub struct CallResult {
    pub payload: String,
    pub path: String,
    pub status_code: u16,
}

/// Controls request pacing so the target API is not flooded into answering
/// with 429. `delay` is how long to pause and `every` how many requests run
/// before each pause kicks in.
#[derive(Debug, Clone, Copy)]
pub struct Throttle {
    pub delay: std::time::Duration,
    pub every: usize,
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            delay: std::time::Duration::ZERO,
            every: 1,
        }
    }
}

/// Drives an OpenAPI spec: collects every operation, builds the input
/// combinations and fires them at the server. Build one with [`Driller::new`],
/// tune it with the setters, then [`run`](Driller::run) it.
pub struct Driller {
    spec: oas3::Spec,
    base_url: Option<String>,
    jwt: Option<String>,
    throttle: Throttle,
}

impl Driller {
    #[must_use]
    pub fn new(spec: oas3::Spec) -> Self {
        Self {
            spec,
            base_url: None,
            jwt: None,
            throttle: Throttle::default(),
        }
    }

    /// Override the server base URL. Takes precedence over the one declared in
    /// the spec.
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Bearer token sent on endpoints that declare a matching security scheme.
    #[must_use]
    pub fn jwt(mut self, jwt: impl Into<String>) -> Self {
        self.jwt = Some(jwt.into());
        self
    }

    /// Space out requests to avoid hitting rate limits.
    #[must_use]
    pub const fn throttle(mut self, throttle: Throttle) -> Self {
        self.throttle = throttle;
        self
    }

    /// Execute all operations in the spec.
    ///
    /// # Errors
    /// Returns the first request error encountered, stopping the run.
    pub async fn run(self) -> Result<Vec<Vec<CallResult>>, reqwest::Error> {
        let Self {
            spec,
            base_url,
            jwt,
            throttle,
        } = self;

        tracing::info!("openapi version: {}", spec.openapi);

        // NOTE: url passed in the command line takes precedence over the one in the openapi schema
        let base_url = base_url.unwrap_or_else(|| retrieve_base_url(&spec));
        let jwt_name = get_jwt_token(&spec);

        let posts = collector::collect_post(&spec);
        let gets = collector::collect_gets(&spec);

        let mut operations = vec![];
        operations.extend_from_slice(gets.as_slice());
        operations.extend_from_slice(posts.as_slice());

        let mut all_results = vec![];

        // Shared across operations so `every` counts requests globally instead
        // of restarting the count for each endpoint.
        let mut pacer = Pacer::new(throttle);

        for p in operations {
            let result = exec_operation(
                &spec,
                p.clone(),
                &base_url,
                (jwt_name.clone(), jwt.clone()),
                &mut pacer,
            )
            .await;
            match result {
                Ok(r) => all_results.push(r),
                Err(e) => {
                    tracing::error!("Error executing operation: {:?}", e);
                    return Err(e);
                }
            }
        }

        Ok(all_results)
    }
}

/// Tracks how many requests have been sent so far and applies the configured
/// throttle. Kept as one value so it can be threaded through the call chain as
/// a single argument.
struct Pacer {
    throttle: Throttle,
    sent: usize,
}

impl Pacer {
    const fn new(throttle: Throttle) -> Self {
        Self { throttle, sent: 0 }
    }

    /// Pause before a request once a full group of `every` requests has already
    /// gone out, so neither the first nor the last request waits for nothing.
    async fn before_request(&mut self) {
        let every = self.throttle.every.max(1);
        if self.sent > 0 && !self.throttle.delay.is_zero() && self.sent.is_multiple_of(every) {
            tokio::time::sleep(self.throttle.delay).await;
        }
        self.sent += 1;
    }
}

fn get_jwt_token(spec: &oas3::Spec) -> Option<String> {
    let components = spec.components.as_ref()?;

    components.security_schemes.iter().find_map(|(k, v)| {
        // Security schemes can be a $ref, but a bearer scheme is always inline
        // in practice, so only inline ones are inspected.
        let oas3::spec::ObjectOrReference::Object(scheme) = v else {
            return None;
        };

        match scheme {
            oas3::spec::SecurityScheme::Http { scheme, .. }
                if scheme.eq_ignore_ascii_case("bearer") =>
            {
                Some(k.clone())
            }
            _ => None,
        }
    })
}

async fn exec_operation(
    spec: &oas3::Spec,
    op: collector::Op,
    base_url: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
    pacer: &mut Pacer,
) -> Result<Vec<CallResult>, reqwest::Error> {
    // An operation without its own `security` inherits the spec-level requirement
    let security = if op.operation.security.is_empty() {
        &spec.security
    } else {
        &op.operation.security
    };

    match op.method.as_str() {
        "GET" => drill_get_endpoint(base_url, &op.path, (jwt_name, jwt), security, pacer).await,
        "POST" => {
            let Some(s) = op.payload else {
                tracing::warn!("No payload found for POST {}", op.path);
                return Ok(vec![]);
            };

            drill_post_endpoint(
                base_url,
                op.path.as_str(),
                &s,
                (jwt_name, jwt),
                security,
                spec,
                pacer,
            )
            .await
        }
        _ => {
            tracing::warn!("Unsupported method: {}", op.method);
            Ok(vec![])
        }
    }
}

async fn drill_get_endpoint(
    base_url: &str,
    path: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
    security: &[oas3::spec::SecurityRequirement],
    pacer: &mut Pacer,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    tracing::info!("GET URL: {}", url);

    let client = reqwest::Client::new();
    let mut req = client.request(reqwest::Method::GET, url.clone());

    if let (Some(jwt_name), Some(jwt)) = (jwt_name.as_ref(), jwt.as_ref()) {
        for ss in security {
            for k in ss.0.keys() {
                if k == jwt_name {
                    req = req.header("Authorization", format!("Bearer {jwt}"));
                }
            }
        }
    }

    let r = req.build().map_err(|e| {
        tracing::error!("Error building request: {:?}", e);
        e
    })?;
    pacer.before_request().await;
    let resp = client.execute(r).await?;

    Ok(vec![CallResult {
        payload: String::new(),
        path: url.clone(),
        status_code: resp.status().as_u16(),
    }])
}

async fn drill_post_endpoint(
    base_url: &str,
    path: &str,
    payload: &oas3::spec::ObjectSchema,
    (jwt_name, jwt): (Option<String>, Option<String>),
    security: &[oas3::spec::SecurityRequirement],
    spec: &oas3::Spec,
    pacer: &mut Pacer,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();

    let mut responses = vec![];

    let mut prop_combinations = {
        let mut digger = digger::Digger::new();
        let root = digger.dig(payload, spec);
        if let Err(e) = root {
            tracing::error!("Error digging the payload: {:?}", e);
            return Ok(vec![]);
        }

        shuffler::do_it(&digger.root)
    };

    // add empty payload
    prop_combinations.push(std::collections::HashMap::new());

    for pp in prop_combinations {
        let s = serde_json::to_string(&pp).unwrap(); // TODO: handle the error

        tracing::info!("Payload: {}", s);

        let mut req = client
            .request(reqwest::Method::POST, url.clone())
            .body(s.clone())
            .header("Content-Type", "application/json"); // TODO: Make this configurable

        if let (Some(jwt_name), Some(jwt)) = (jwt_name.as_ref(), jwt.as_ref()) {
            for ss in security {
                for k in ss.0.keys() {
                    if k == jwt_name {
                        req = req.header("Authorization", format!("Bearer {jwt}"));
                    }
                }
            }
        }

        let r = req.build().unwrap(); // TODO: handle the error
        pacer.before_request().await;
        let resp = client.execute(r).await?;

        tracing::info!("Response: {:?}", resp);

        responses.push(CallResult {
            payload: s,
            path: url.clone(),
            status_code: resp.status().as_u16(),
        });
    }

    Ok(responses)
}

fn retrieve_base_url(spec: &oas3::Spec) -> String {
    spec.servers.first().map_or_else(
        || {
            tracing::error!("No servers found in the openapi schema");
            std::process::exit(1);
        },
        |s| {
            // When the server URL is templated, fall back to the first
            // variable's default value (which holds the concrete URL here).
            s.variables
                .iter()
                .next()
                .map_or_else(|| s.url.clone(), |(_, var)| var.default.clone())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_base_url() {
        {
            // easy
            let s = std::include_str!("./testdata/single_server.yml");
            let spec = parse_openapi(s).unwrap();

            let base = retrieve_base_url(&spec);
            assert_eq!(base, "http://127.0.0.1:8000");
        }
        {
            // server from env
            let s = std::include_str!("./testdata/server_from_env.yml");
            let spec = parse_openapi(s).unwrap();
            let base = retrieve_base_url(&spec);
            assert_eq!(base, "http://localhost:8000"); // pick the default one
        }
    }

    // This is a fake test to make sure the test suite is setup with tracing.
    #[test]
    fn fake_test() {
        tracing_subscriber::fmt::init();
        assert!(true);
    }

    #[test]
    fn find_jwt_token_in_components() {
        let s = std::include_str!("./testdata/get_more_info_with_jwt.yml");
        let spec = parse_openapi(s).unwrap();
        let jwt = get_jwt_token(&spec);

        assert!(jwt.is_some());
        assert_eq!(jwt.unwrap(), "bearerAuth");
    }

    #[test]
    fn jwt_token_not_found_when_no_bearer_scheme() {
        // Test with a spec that has no security schemes
        let s = std::include_str!("./testdata/get_info.yml");
        let spec = parse_openapi(s).unwrap();
        let jwt = get_jwt_token(&spec);

        assert!(jwt.is_none());
    }

    #[test]
    fn retrieve_base_url_with_multiple_servers() {
        // When there are multiple servers, it should pick the first one
        let s = std::include_str!("./testdata/multiple_servers.yml");
        let spec = parse_openapi(s).unwrap();
        let base = retrieve_base_url(&spec);

        assert_eq!(base, "http://first.example.com");
    }

    #[test]
    fn jwt_token_ignored_for_non_bearer_http_scheme() {
        // A basic-auth scheme is HTTP but not bearer, so no JWT name is found.
        let s = std::include_str!("./testdata/get_info_basic_auth.yml");
        let spec = parse_openapi(s).unwrap();

        assert!(get_jwt_token(&spec).is_none());
    }

    #[test]
    fn jwt_scheme_matching_is_case_insensitive() {
        // The bearer scheme name is matched ignoring ASCII case.
        let s = std::include_str!("./testdata/get_more_info_with_jwt.yml");
        let spec = parse_openapi(s).unwrap();

        assert_eq!(get_jwt_token(&spec).unwrap(), "bearerAuth");
    }
}
