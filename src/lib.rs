mod collector;
mod digger;
mod shuffler;

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
    openapi_schema: openapiv3::OpenAPI,
    url: Option<String>,
    jwt: Option<String>,
) -> Result<Vec<Vec<CallResult>>, reqwest::Error> {
    tracing::info!("openapi version: {}", openapi_schema.openapi);

    let base_url = retrieve_base_url(&openapi_schema);

    let components = openapi_schema.components.map_or_else(
        || {
            tracing::error!("No components found in the openapi schema");
            std::process::exit(1);
        },
        |c| c,
    );

    // NOTE: url passed in the command line takes precedence over the one in the openapi schema
    let base_url = url.map_or(base_url, |b| b);
    let jwt_name = get_jwt_token(&components);

    let posts = collector::collect_post(&openapi_schema.paths, &components);
    let gets = collector::collect_gets(&openapi_schema.paths);

    let mut operations = vec![];
    operations.extend_from_slice(gets.as_slice());
    operations.extend_from_slice(posts.as_slice());

    let mut all_results = vec![];

    for p in operations {
        let result = exec_operation(
            &components,
            p.clone(),
            &base_url,
            (jwt_name.clone(), jwt.clone()),
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

fn get_jwt_token(components: &openapiv3::Components) -> Option<String> {
    components
        .security_schemes
        .iter()
        .filter_map(|(k, v)| {
            let v = v.as_item();
            v.map(|scheme| (k.clone(), scheme.clone()))
        })
        .find_map(|(k, v)| match v {
            openapiv3::SecurityScheme::HTTP {
                scheme, // FIXME: do we need to validate other fields here?
                ..
            } if scheme.to_lowercase() == "bearer" => Some(k),
            _ => None,
        })
}

async fn exec_operation(
    components: &openapiv3::Components,
    op: collector::Op,
    base_url: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
) -> Result<Vec<CallResult>, reqwest::Error> {
    match op.method.as_str() {
        "GET" => {
            drill_get_endpoint(base_url, &op.path, (jwt_name, jwt), op.operation.security).await
        }
        "POST" => {
            let Some(s) = op.payload else {
                tracing::warn!("No payload found for POST {}", op.path);
                return Ok(vec![]);
            };

            drill_post_endpoint(
                base_url,
                op.path.as_str(),
                s,
                (jwt_name, jwt),
                op.operation.security,
                components,
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
    security: Option<Vec<openapiv3::SecurityRequirement>>,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    tracing::info!("GET URL: {}", url);

    let client = reqwest::Client::new();
    let mut req = client.request(reqwest::Method::GET, url.clone());

    if let Some(s) = security
        && jwt.is_some()
        && jwt_name.is_some()
    {
        for ss in s {
            for (k, _) in ss {
                let jwt_name = jwt_name.clone().unwrap();
                let jwt = jwt.clone().unwrap();

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
        path: url.to_string(),
        status_code: resp.status().as_u16(),
    }])
}

async fn drill_post_endpoint(
    base_url: &str,
    path: &str,
    payload: openapiv3::Schema,
    (jwt_name, jwt): (Option<String>, Option<String>),
    security: Option<Vec<openapiv3::SecurityRequirement>>,
    components: &openapiv3::Components,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();

    let mut responses = vec![];

    let mut prop_combinations = {
        let mut digger = digger::Digger::new();
        let root = digger.dig(payload, components);
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

        if let Some(ref s) = security
            && jwt.is_some()
            && jwt_name.is_some()
        {
            for ss in s {
                for (k, _) in ss {
                    let jwt_name = jwt_name.clone().unwrap();
                    let jwt = jwt.clone().unwrap();

                    if k == &jwt_name {
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
            path: url.to_string(),
            status_code: resp.status().as_u16(),
        });
    }

    Ok(responses)
}

fn retrieve_base_url(openapi_schema: &openapiv3::OpenAPI) -> String {
    openapi_schema.servers.first().map_or_else(
        || {
            tracing::error!("No servers found in the openapi schema");
            std::process::exit(1);
        },
        |s| {
            s.variables.as_ref().map_or_else(
                || s.url.clone(),
                |v| {
                    let f = v.first().unwrap();
                    f.1.default.clone()
                },
            )
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
            let openapi_schema = &serde_yaml_bw::from_str(s).unwrap();

            let base = retrieve_base_url(openapi_schema);
            assert_eq!(base, "http://127.0.0.1:8000");
        }
        {
            // server from env
            let s = std::include_str!("./testdata/server_from_env.yml");
            let openapi_schema = &serde_yaml_bw::from_str(s).unwrap();
            let base = retrieve_base_url(openapi_schema);
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
        let openapi_schema = serde_yaml_bw::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let components = openapi_schema.components.unwrap();
        let jwt = get_jwt_token(&components);

        assert!(jwt.is_some());
        assert_eq!(jwt.unwrap(), "bearerAuth");
    }

    #[test]
    fn jwt_token_not_found_when_no_bearer_scheme() {
        // Test with a spec that has no security schemes
        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let components = openapi_schema.components.unwrap();
        let jwt = get_jwt_token(&components);

        assert!(jwt.is_none());
    }

    #[test]
    fn retrieve_base_url_with_multiple_servers() {
        // When there are multiple servers, it should pick the first one
        let s = std::include_str!("./testdata/single_server.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let base = retrieve_base_url(&openapi_schema);

        // Should use the first server's URL
        assert_eq!(base, "http://127.0.0.1:8000");
    }
}
