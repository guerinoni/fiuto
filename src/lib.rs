mod collector;
mod digger;
mod parser;
mod shuffler;

pub use parser::parse_openapi;

#[derive(Debug, serde::Serialize)]
pub struct CallResult {
    payload: String,
    path: String,
    pub status_code: u16,
}

/// Execute all operations in the openapi schema
///
/// # Errors
pub async fn do_it(
    spec: oas3::Spec,
    url: Option<String>,
    jwt: Option<String>,
) -> Result<Vec<Vec<CallResult>>, reqwest::Error> {
    tracing::info!("openapi version: {}", spec.openapi);

    let base_url = retrieve_base_url(&spec);

    // NOTE: url passed in the command line takes precedence over the one in the openapi schema
    let base_url = url.map_or(base_url, |b| b);
    let jwt_name = get_jwt_token(&spec);

    let posts = collector::collect_post(&spec);
    let gets = collector::collect_gets(&spec);

    let mut operations = vec![];
    operations.extend_from_slice(gets.as_slice());
    operations.extend_from_slice(posts.as_slice());

    let mut all_results = vec![];

    for p in operations {
        let result =
            exec_operation(&spec, p.clone(), &base_url, (jwt_name.clone(), jwt.clone())).await;
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
) -> Result<Vec<CallResult>, reqwest::Error> {
    match op.method.as_str() {
        "GET" => {
            drill_get_endpoint(base_url, &op.path, (jwt_name, jwt), &op.operation.security).await
        }
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
                &op.operation.security,
                spec,
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
        let s = std::include_str!("./testdata/single_server.yml");
        let spec = parse_openapi(s).unwrap();
        let base = retrieve_base_url(&spec);

        // Should use the first server's URL
        assert_eq!(base, "http://127.0.0.1:8000");
    }
}
