use anyhow::Context;
use axum::Router;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uc_api::{catalog_api, control_api, delta_api, middleware::auth_middleware, state::AppState};
use uc_auth::{AllowingAuthorizer, JwkSet, JwtConfig, KeyManager, OidcConfig, UcAuthorizer};
use uc_credentials::CloudCredentialVendor;
use uc_db::{
    pool::run_migrations,
    repos::{MetastoreRepo, UserRepo},
    AnyPool,
};
use uuid::Uuid;

// ── CLI args ──────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "uc-server", about = "Unity Catalog server (Rust)")]
struct Args {
    #[arg(long, default_value_t = 8080)]
    port: u16,

    #[arg(long, default_value = "./etc/conf")]
    config_dir: PathBuf,

    #[arg(long, default_value = "sqlite:./etc/db/uc.db?mode=rwc")]
    database_url: String,

    /// Disable authorization (allow all requests). Use only in dev/testing.
    #[arg(long, default_value_t = false)]
    no_auth: bool,

    /// OIDC issuer URL. When set, Bearer tokens issued by this issuer are
    /// accepted via JWKS validation (fetched from {issuer}/.well-known/...).
    /// Intended for in-cluster K8s SA projected tokens.
    #[arg(long)]
    oidc_issuer: Option<String>,

    /// Vend real AWS STS-assumed credentials for S3-scheme storage
    /// credentials (temporary-table/path-credentials APIs), instead of
    /// returning Unimplemented for the S3 scheme. Requires UC-server's own
    /// AWS identity to have sts:AssumeRole permission on each
    /// StorageCredential's role_arn. On by default -- every deployment in
    /// this project uses MinIO/S3-compatible storage, so this is the normal
    /// path, not an opt-in; pass --enable-aws-credentials=false to disable.
    #[arg(long, default_value_t = true)]
    enable_aws_credentials: bool,

    /// Deterministic OIDC `sub` of a "bootstrap operator" principal to grant
    /// OWNER on the metastore at startup (mirrors the admin@unitycatalog.io
    /// bootstrap below, but keyed by external_id instead of email). Useful
    /// when uc-server is deployed alongside automation that bootstraps catalogs
    /// using K8s SA projected tokens. Each automating service authenticates
    /// with a projected K8s SA token carrying a deterministic `sub` of the form
    /// `system:serviceaccount:<namespace>:<service-account-name>` — passing
    /// that string here lets those bootstrap calls succeed instead of
    /// failing with 403, since brand-new OIDC principals otherwise get zero
    /// grants. Repeatable (comma-separated or multiple flags) to cover more
    /// than one bootstrapping identity. Can also be set via the
    /// OPERATOR_EXTERNAL_ID env var (comma-separated for multiple values).
    /// Unset by default — zero behavior change for any deployment that
    /// doesn't pass it (local dev, tests, --no-auth setups).
    #[arg(long, env = "OPERATOR_EXTERNAL_ID", value_delimiter = ',')]
    operator_external_id: Vec<String>,

    #[arg(long, default_value = "info")]
    log_level: String,
}

fn build_credential_vendor(enable_aws: bool) -> CloudCredentialVendor {
    if enable_aws {
        CloudCredentialVendor::with_aws()
    } else {
        CloudCredentialVendor::new()
    }
}

/// Grant each deterministic-`sub` operator principal OWNER on the metastore
/// at startup, mirroring the admin@unitycatalog.io bootstrap above but keyed
/// by OIDC `external_id` rather than email (OIDC principals created via
/// `find_or_create_by_external_id` have `email: None` and can never match
/// the admin-by-email lookup). No-op when `external_ids` is empty (the
/// default) — existing deployments that don't pass
/// `--operator-external-id`/`OPERATOR_EXTERNAL_ID` are unaffected.
///
/// Once granted OWNER on the metastore, each principal's own catalog/schema/
/// table creation calls succeed: `authorize_any(.., [CreateCatalog, Owner])`
/// passes, and each creation handler explicitly grants the creator OWNER on
/// the newly created object (see uc-api's catalogs/schemas/tables `create`
/// handlers), so no further per-object grants are needed here.
async fn bootstrap_operator_principal(
    pool: &AnyPool,
    authorizer: &dyn uc_auth::Authorizer,
    metastore_id: Uuid,
    external_ids: &[String],
) -> anyhow::Result<()> {
    for external_id in external_ids {
        let user = UserRepo::find_or_create_by_external_id(pool, external_id)
            .await
            .context("Failed to find_or_create operator principal")?;

        let already_owner = authorizer
            .authorize(user.id, metastore_id, uc_types::Privilege::Owner)
            .await
            .context("Failed to check operator principal's existing grants")?;

        if !already_owner {
            authorizer
                .grant(user.id, metastore_id, uc_types::Privilege::Owner)
                .await
                .context("Failed to grant operator principal OWNER on metastore")?;
            info!(
                external_id,
                "Granted OWNER on metastore to operator principal"
            );
        }
    }

    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("uc_server={l},uc_api={l},uc_db={l}", l = args.log_level).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();

    info!("Starting Unity Catalog server on port {}", args.port);
    info!("Config dir: {}", args.config_dir.display());
    info!("Database:   {}", mask_db_url(&args.database_url));
    info!(
        "Auth:       {}",
        if args.no_auth { "disabled" } else { "enabled" }
    );

    // ── 1. RSA key initialization ─────────────────────────────────────────────
    let key_manager =
        KeyManager::load_or_generate(&args.config_dir).context("Failed to initialize RSA keys")?;
    let jwt_config = JwtConfig::from_der(
        &key_manager.private_key_der,
        &key_manager.public_key_der,
        key_manager.key_id.clone(),
    )
    .context("Failed to create JWT config")?;

    // ── 2. Database pool + migrations ─────────────────────────────────────────
    let (actual_db_url, s3_info) = prepare_database_url(&args.config_dir, &args.database_url)
        .await
        .context("Failed to prepare database URL")?;

    let pool = uc_db::pool::connect(&actual_db_url)
        .await
        .context("Failed to connect to database")?;

    run_migrations(&pool)
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations applied");

    // ── 3. Metastore initialization ───────────────────────────────────────────
    let metastore = MetastoreRepo::get_or_init(&pool, "unity-catalog")
        .await
        .context("Failed to initialize metastore")?;
    let metastore_id = metastore.id;
    info!("Metastore ID: {}", metastore_id);

    // ── 4. Authorization ──────────────────────────────────────────────────────
    let authorizer: Arc<dyn uc_auth::Authorizer> = if args.no_auth {
        info!("Authorization disabled — all requests allowed");
        Arc::new(AllowingAuthorizer)
    } else {
        info!("Authorization enabled — loading casbin policies from DB");
        let uc_auth = UcAuthorizer::new_with_db(pool.clone())
            .await
            .context("Failed to initialize casbin authorizer")?;
        Arc::new(uc_auth)
    };

    // ── 5. Admin user initialization ──────────────────────────────────────────
    let admin_email = "admin@unitycatalog.io";
    if !args.no_auth {
        if UserRepo::get_by_email(&pool, admin_email).await?.is_none() {
            // UUIDv7: time-ordered — encodes when this admin user was created
            let admin_id = Uuid::now_v7();
            let now = chrono::Utc::now().timestamp_millis();
            UserRepo::create(
                &pool,
                admin_id,
                admin_email,
                Some(admin_email),
                None,
                "ENABLED",
                now,
            )
            .await
            .context("Failed to create admin user")?;
            authorizer
                .grant(admin_id, metastore_id, uc_types::Privilege::Owner)
                .await
                .context("Failed to grant admin OWNER on metastore")?;
            info!("Created admin user: {}", admin_email);
        }
    }

    // ── 5b. Operator bootstrap principal (Tier 2 auto-provisioning) ───────────
    if !args.no_auth {
        bootstrap_operator_principal(
            &pool,
            authorizer.as_ref(),
            metastore_id,
            &args.operator_external_id,
        )
        .await
        .context("Failed to bootstrap operator principal")?;
    }

    // ── 6. Admin token (write to config_dir for local dev convenience) ────────
    let token_claims = uc_auth::jwt::UcClaims::new_access(admin_email);
    let token = uc_auth::jwt::encode_token(&jwt_config, &token_claims)
        .context("Failed to create admin token")?;
    std::fs::write(args.config_dir.join("token.txt"), &token)
        .context("Failed to write token.txt")?;

    // ── 7. OIDC setup (optional; skipped when --no-auth) ─────────────────────
    let oidc_config = if !args.no_auth {
        if let Some(ref issuer) = args.oidc_issuer {
            let jwks = fetch_oidc_jwks(issuer)
                .await
                .context("Failed to fetch OIDC JWKS")?;
            info!("OIDC auth enabled, issuer: {}", issuer);
            Some(Arc::new(OidcConfig {
                issuer: issuer.clone(),
                jwks,
            }))
        } else {
            None
        }
    } else {
        None
    };

    // ── 8. App state ──────────────────────────────────────────────────────────
    let state = AppState::new(
        pool,
        authorizer,
        build_credential_vendor(args.enable_aws_credentials),
        jwt_config,
        metastore_id,
        !args.no_auth,
        args.config_dir.clone(),
        oidc_config,
    );

    // ── 9. Router assembly ────────────────────────────────────────────────────
    let app = Router::new()
        .merge(catalog_api::router(state.clone()))
        .merge(control_api::router(state.clone()))
        .merge(delta_api::router(state.clone()))
        .route(
            "/",
            axum::routing::get(|| async { "Hello, Unity Catalog!" }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        // Registered after the auth layer so it is *not* wrapped by it: an
        // unauthenticated liveness/readiness endpoint that validates the HTTP
        // stack is up (unlike a tcpSocket probe). Returns 200 OK, no auth.
        .route("/health", axum::routing::get(|| async { "OK" }));

    // ── 10. Bind and serve ────────────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to port")?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    if let Some(ref s3_info) = s3_info {
        let s3_client = build_s3_client().ok();
        let bucket = s3_info.bucket.clone();
        let key = s3_info.key.clone();
        let local_path = s3_info.local_path.clone();
        tokio::spawn(async move {
            let _ = shutdown_rx.await;
            if let Some(client) = s3_client {
                let endpoint = std::env::var("AWS_ENDPOINT_URL").unwrap_or_default();
                let url = if endpoint.is_empty() {
                    format!("https://{bucket}.s3.amazonaws.com/{key}")
                } else {
                    format!("{endpoint}/{bucket}/{key}")
                };
                if let Ok(data) = tokio::fs::read(&local_path).await {
                    let _ = client.put(&url).body(data).send().await;
                    info!("Uploaded metadata DB to s3://{bucket}/{key}");
                }
            }
        });
    }

    axum::serve(listener, app).await.context("Server error")?;

    let _ = shutdown_tx.send(());
    Ok(())
}

// ── S3-backed SQLite support ────────────────────────────────────────────────────

#[allow(dead_code)]
struct S3Info {
    bucket: String,
    pub key: String,
    pub local_path: PathBuf,
}

async fn prepare_database_url(
    config_dir: &PathBuf,
    url: &str,
) -> anyhow::Result<(String, Option<S3Info>)> {
    let Some(stripped) = url.strip_prefix("s3://") else {
        std::fs::create_dir_all("./etc/db").ok();
        return Ok((url.to_string(), None));
    };

    let (bucket, key) = match stripped.split_once('/') {
        Some((b, k)) => (b.to_string(), k.to_string()),
        None => anyhow::bail!("Invalid s3:// URL: expected s3://bucket/key, got {url}"),
    };

    let s3_client = build_s3_client()?;
    let local_dir = config_dir.join("db");
    std::fs::create_dir_all(&local_dir)?;
    let local_path = local_dir.join(format!("uc-{}.db", sanitize_filename(&key)));

    download_from_s3(&s3_client, &bucket, &key, &local_path).await?;
    info!("Downloaded metadata DB from s3://{bucket}/{key}");

    let sqlite_url = format!("sqlite:{}?mode=rwc", local_path.display());
    Ok((
        sqlite_url,
        Some(S3Info {
            bucket,
            key,
            local_path,
        }),
    ))
}

fn build_s3_client() -> anyhow::Result<reqwest::Client> {
    let endpoint = std::env::var("AWS_ENDPOINT_URL").unwrap_or_default();
    let key_id = std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default();
    let secret = std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default();

    let mut builder = reqwest::Client::builder();
    if !endpoint.is_empty() {
        let auth = format!("{}:{}", key_id, secret);
        let encoded = base64_encode(&auth);
        let header_value = format!("Basic {encoded}");
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(hv) = reqwest::header::HeaderValue::from_str(&header_value) {
            headers.insert(reqwest::header::AUTHORIZATION, hv);
        }
        builder = builder.default_headers(headers);
    }
    Ok(builder.build()?)
}

async fn download_from_s3(
    client: &reqwest::Client,
    bucket: &str,
    key: &str,
    local_path: &std::path::Path,
) -> anyhow::Result<()> {
    if local_path.exists() {
        return Ok(());
    }
    let endpoint = std::env::var("AWS_ENDPOINT_URL").unwrap_or_default();
    let url = if endpoint.is_empty() {
        format!("https://{bucket}.s3.amazonaws.com/{key}")
    } else {
        format!("{endpoint}/{bucket}/{key}")
    };
    let resp = client.get(&url).send().await?;
    if resp.status().is_success() {
        let bytes = resp.bytes().await?;
        tokio::fs::write(local_path, &bytes).await?;
    }
    Ok(())
}

fn sanitize_filename(key: &str) -> String {
    key.replace('/', "_")
}

/// Redact the `user:password@` credentials from a database URL before logging.
/// SQLite URLs have no credentials and pass through unchanged; Postgres/S3
/// URLs of the form `scheme://user:pass@host/...` have the password masked.
fn mask_db_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    // Only the authority (up to the first '/') can carry credentials.
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (rest, None),
    };
    let masked_authority = match authority.split_once('@') {
        Some((creds, host)) => {
            let user = creds.split_once(':').map(|(u, _)| u).unwrap_or(creds);
            format!("{user}:****@{host}")
        }
        None => authority.to_string(),
    };
    match path {
        Some(p) => format!("{scheme}://{masked_authority}/{p}"),
        None => format!("{scheme}://{masked_authority}"),
    }
}

fn base64_encode(input: &str) -> String {
    let mut buf = Vec::new();
    for chunk in input.as_bytes().chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
        let n = (b0 as u32) << 16 | (b1 as u32) << 8 | (b2 as u32);
        buf.push(ENC_TABLE[((n >> 18) & 63) as usize]);
        buf.push(ENC_TABLE[((n >> 12) & 63) as usize]);
        buf.push(ENC_TABLE[((n >> 6) & 63) as usize]);
        buf.push(ENC_TABLE[(n & 63) as usize]);
    }
    let padding = (3 - input.as_bytes().len() % 3) % 3;
    for _ in 0..padding {
        buf.pop();
        buf.push(b'=');
    }
    String::from_utf8(buf).unwrap()
}

const ENC_TABLE: [u8; 64] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// ── OIDC JWKS discovery ───────────────────────────────────────────────────────

async fn fetch_oidc_jwks(issuer: &str) -> anyhow::Result<JwkSet> {
    let issuer = issuer.trim_end_matches('/');
    let discovery_url = format!("{issuer}/.well-known/openid-configuration");

    // In-cluster: load the k8s CA cert and SA bearer token from the automounted SA volume.
    // - CA cert: lets reqwest/rustls verify the k3s API server's self-signed cert.
    // - Bearer token: k3s requires auth on /.well-known/openid-configuration (returns 401 otherwise).
    let mut builder = reqwest::Client::builder();
    if let Ok(pem) = std::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt") {
        if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder.build().context("Failed to build HTTP client")?;

    let sa_token = std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
        .unwrap_or_default();
    let sa_token = sa_token.trim();

    let mut discovery_req = client.get(&discovery_url);
    if !sa_token.is_empty() {
        discovery_req = discovery_req.bearer_auth(sa_token);
    }
    let discovery: serde_json::Value = discovery_req
        .send()
        .await
        .context("OIDC discovery request failed")?
        .json()
        .await
        .context("OIDC discovery response not valid JSON")?;

    let jwks_uri = discovery["jwks_uri"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("OIDC discovery response missing 'jwks_uri'"))?;

    let mut jwks_req = client.get(jwks_uri);
    if !sa_token.is_empty() {
        jwks_req = jwks_req.bearer_auth(sa_token);
    }
    let jwks: JwkSet = jwks_req
        .send()
        .await
        .context("JWKS fetch failed")?
        .json()
        .await
        .context("JWKS response not valid JSON")?;
    Ok(jwks)
}
