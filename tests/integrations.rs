use axum::RequestPartsExt;

#[derive(Clone)]
struct AppState {}

async fn run_api() -> String {
    let state = std::sync::Arc::new(AppState {});
    let app = axum::Router::new()
        .route("/api/v1/org/info", axum::routing::get(info))
        .route("/api/v1/org/more/info", axum::routing::get(more_info))
        .route("/api/v1/org/login", axum::routing::post(login_handler))
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

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        use axum_extra::headers::authorization::Bearer;
        use axum_extra::headers::Authorization;

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
    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());
    let r = r.unwrap();

    assert_eq!(r.len(), 1);

    // TODO: add more checks about code returned/expected
}

#[tokio::test]
async fn post_login() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_login.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());
    let r = r.unwrap();

    assert_eq!(r.len(), 1); // 1 endpoint

    let combinations = r.get(0).unwrap();
    assert_eq!(combinations.len(), 8);

    // TODO: add more checks about code returned/expected
}

#[tokio::test]
async fn get_with_jwt() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/get_more_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
    let token = Some("test_token_get_with_jwt".to_owned());
    let r = fiuto::do_it(openapi_schema, Some(url), token).await;

    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(r.len(), 1);

    // TODO: Add more check, specially about the token, but we need responses more info, not just code.
}

#[tokio::test]
async fn post_with_jwt() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_info_with_jwt.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
    let token = Some("test_token_post_with_jwt".to_owned());
    let r = fiuto::do_it(openapi_schema, Some(url), token).await;

    assert!(r.is_ok());
}

#[tokio::test]
async fn post_with_nested_property_body() {
    let url = run_api().await;

    let s = std::include_str!("../src/testdata/post_info_nested_property.yml");
    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
    let r = fiuto::do_it(openapi_schema, Some(url), None).await;

    assert!(r.is_ok());
}
