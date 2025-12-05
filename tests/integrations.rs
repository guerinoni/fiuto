use axum::RequestPartsExt;

#[derive(Clone)]
struct AppState {}

async fn run_api() -> String {
    let state = std::sync::Arc::new(AppState {});
    let app = axum::Router::new()
        .route("/api/v1/org/info", axum::routing::get(info))
        .route("/api/v1/org/more/info", axum::routing::get(more_info))
        .route("/api/v1/org/login", axum::routing::post(login_handler))
        .route("/api/v1/login", axum::routing::post(login_handler))
        .route("/api/v1/org/info", axum::routing::post(post_info))
        .route("/api/v1/org/hq", axum::routing::post(post_hq))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();

    let base_url = format!("http://{}", listener.local_addr().unwrap());
    tracing::info!("listening on {}", base_url);

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    base_url
}

/// LoginRequest is the body expected for a simple login request.
#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct LoginRequest {
    email: String,
    password: String,
    org: String,
}

async fn login_handler(
    axum::extract::State(_): axum::extract::State<std::sync::Arc<AppState>>,
    axum::Json(payload): axum::Json<LoginRequest>,
) -> axum::Json<String> {
    tracing::info!("Received login request: {:?}", payload);
    axum::Json(format!(
        "User {} from org {} is trying to login",
        payload.email, payload.org
    ))
}

pub enum AuthError {
    TokenNotFound,
}

// This allows to be used as the `Rejection` type in the `FromRequestPars` trait.
impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, error_message) = match self {
            Self::TokenNotFound => (StatusCode::UNAUTHORIZED, "Token not found"),
        };
        let body = axum::Json(serde_json::json!({
            "error": error_message,
        }));
        (status, body).into_response()
    }
}

impl<S> axum::extract::FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        use axum_extra::headers::Authorization;
        use axum_extra::headers::authorization::Bearer;

        let axum_extra::TypedHeader(Authorization(bearer)) = parts
            .extract::<axum_extra::TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AuthError::TokenNotFound)?;

        tracing::info!("header bearer: {:?}", bearer);

        let token_data = Claims {
            token_received: bearer.token().to_owned(),
        };

        Ok(token_data)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Claims {
    token_received: String,
}

async fn info() -> axum::Json<&'static str> {
    axum::Json("Hello, World!")
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct InfoRequest {
    address: String,
}

async fn post_info(
    claims: Claims,
    axum::Json(payload): axum::Json<InfoRequest>,
) -> axum::Json<String> {
    tracing::info!("post info received token: {}", claims.token_received);
    let _ = payload;
    axum::Json(claims.token_received)
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct Info {
    hq: HQ,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct HQ {
    address: String,
    postal_code: String,
    city: String,
    country: String,
    state_region: String,
}

async fn post_hq(axum::Json(payload): axum::Json<Info>) -> axum::Json<String> {
    tracing::info!("post hq received: {:?}", payload);
    axum::Json("ok".to_string())
}

// this return the token populated during the request, this way we can use it for test checks.
async fn more_info(claims: Claims) -> axum::Json<String> {
    axum::Json(claims.token_received)
}

/// This is a fake test to make sure the test suite is setup with tracing.
#[test]
fn fake_test() {
    tracing_subscriber::fmt::init();
    assert!(true);
}

#[tokio::test]
async fn get_info_simple() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/get_info.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());
    let r = r.unwrap();

    assert_eq!(r.len(), 1);

    let endpoint_results = r.first().unwrap();
    assert_eq!(endpoint_results.len(), 1);
    // GET without auth should succeed
    assert_eq!(endpoint_results.first().unwrap().status_code, 200);
}

#[tokio::test]
async fn post_login() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_login.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());
    let r = r.unwrap();

    assert_eq!(r.len(), 1); // 1 endpoint

    let combinations = r.first().unwrap();
    assert_eq!(combinations.len(), 8); // 2^3 - 1 combinations + 1 empty payload

    // only the full payload (all 3 fields) should succeed with 200
    // partial/empty payloads should fail with 422 (Unprocessable Entity)
    let success_count = combinations
        .iter()
        .filter(|c| c.status_code == 200)
        .count();
    let error_count = combinations
        .iter()
        .filter(|c| c.status_code == 422)
        .count();

    assert_eq!(success_count, 1, "Only complete payload should succeed");
    assert_eq!(error_count, 7, "Incomplete payloads should return 422");
}

#[tokio::test]
async fn get_with_jwt() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/get_more_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let token = Some("test_token_get_with_jwt".to_owned());
    let r = fiuto::do_it(openapi_schema, Some(url), token).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    let endpoint_results = r.first().unwrap();
    assert_eq!(endpoint_results.len(), 1);
    // GET with valid JWT should succeed
    assert_eq!(endpoint_results.first().unwrap().status_code, 200);
}

#[tokio::test]
async fn post_with_jwt() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let token = Some("test_token_post_with_jwt".to_owned());
    let r = fiuto::do_it(openapi_schema, Some(url), token).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    let combinations = r.first().unwrap();
    // infoRequest has 1 property (address), so 2^1 - 1 + 1 empty = 2 combinations
    assert_eq!(combinations.len(), 2);

    // with valid JWT: complete payload should succeed, empty should fail
    let success_count = combinations
        .iter()
        .filter(|c| c.status_code == 200)
        .count();
    assert_eq!(success_count, 1, "Complete payload with JWT should succeed");
}

#[tokio::test]
async fn post_with_nested_property_body() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_info_nested_property.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    let combinations = r.first().unwrap();
    // nested HQ has 5 properties, so combinations will be generated for those
    assert!(!combinations.is_empty());

    // only complete nested payload should succeed
    let success_count = combinations
        .iter()
        .filter(|c| c.status_code == 200)
        .count();
    assert!(success_count >= 1, "Complete nested payload should succeed");
}

#[tokio::test]
async fn get_without_jwt_returns_401() {
    let url = run_api().await;

    // this spec requires JWT but we don't provide one
    let s = std::include_str!("../src/testdata/get_more_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    let endpoint_results = r.first().unwrap();
    assert_eq!(endpoint_results.len(), 1);
    // GET without required JWT should return 401 Unauthorized
    assert_eq!(endpoint_results.first().unwrap().status_code, 401);
}

#[tokio::test]
async fn post_without_jwt_returns_401() {
    let url = run_api().await;

    // this spec requires JWT but we don't provide one
    let s = std::include_str!("../src/testdata/post_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    let combinations = r.first().unwrap();
    // all requests should fail with 401 since no JWT provided
    assert!(
        combinations.iter().all(|c| c.status_code == 401),
        "All requests without JWT should return 401"
    );
}
