use anyhow::Context;
use axum::Router;
use clap::Parser;
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uc_api::{catalog_api, control_api, delta_api, middleware::auth_middleware, state::AppState};
use uc_auth::{AllowingAuthorizer, JwkSet, OidcConfig, UcAuthorizer};
use uc_credentials::CloudCredentialVendor;
use uc_db::{
    repos::{metastore, user},
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

    /// Seconds between background refreshes of the in-memory snapshot from the
    /// log. 0 disables it.
    ///
    /// Only matters with more than one uc-server on the same log. A replica
    /// learns about another's commits when it next writes (the conditional PUT
    /// forces a replay on conflict), but a read-only replica would otherwise
    /// serve stale metadata indefinitely. This bounds that staleness at the
    /// cost of one LIST per interval.
    ///
    /// It does not make reads linearizable — a request can still land in the
    /// window between another replica's commit and the next refresh. It turns
    /// unbounded staleness into bounded staleness, which is a different and
    /// weaker claim.
    #[arg(long, default_value_t = 0)]
    refresh_interval_secs: u64,

    /// Object-store root for the log-structured metadata store, as
    /// `s3://bucket/prefix`. There is no database, no migrations and no volume
    /// to lose.
    #[arg(long, default_value = "")]
    storage_root: String,

    /// OIDC issuer URL. Bearer tokens from this issuer are accepted, validated
    /// against its JWKS.
    ///
    /// This single flag decides authentication: set, and auth and RBAC are
    /// enforced; unset, and every request is allowed. They were two flags, and
    /// "auth enabled with no issuer" was a startup that accepted nothing --
    /// UC signs no tokens of its own, so the issuer is the only trust root.
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
        let user = user::find_or_create_by_external_id(pool, external_id)
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
    info!("Metadata:   {}", args.storage_root);
    info!(
        "Auth:       {}",
        if args.oidc_issuer.is_some() { "enabled" } else { "disabled" }
    );

    // ── 1. Store ──────────────────────────────────────────────────────────────
    let pool = {
        let pool = open_log_store(&args.storage_root)
            .await
            .context("Failed to open the metadata log")?;
        info!(
            "Log store replayed to version {}",
            pool.snapshot().await.version
        );
        pool
    };

    // ── 3. Metastore initialization ───────────────────────────────────────────
    let metastore = metastore::get_or_init(&pool, "unity-catalog")
        .await
        .context("Failed to initialize metastore")?;
    let metastore_id = metastore.id;
    info!("Metastore ID: {}", metastore_id);

    // ── 4. Authorization ──────────────────────────────────────────────────────
    let authorizer: Arc<dyn uc_auth::Authorizer> = if args.oidc_issuer.is_none() {
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
    if args.oidc_issuer.is_some() && user::get_by_email(&pool, admin_email).await?.is_none() {
        // UUIDv7: time-ordered — encodes when this admin user was created
        let admin_id = Uuid::now_v7();
        let now = chrono::Utc::now().timestamp_millis();
        user::create(
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

    // ── 5b. Operator bootstrap principal (Tier 2 auto-provisioning) ───────────
    if args.oidc_issuer.is_some() {
        bootstrap_operator_principal(
            &pool,
            authorizer.as_ref(),
            metastore_id,
            &args.operator_external_id,
        )
        .await
        .context("Failed to bootstrap operator principal")?;
    }

    // ── 7. OIDC setup (optional; skipped when --no-auth) ─────────────────────
    let oidc_config = {
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
    };

    // ── 8b. Snapshot refresh ──────────────────────────────────────────────────
    if args.refresh_interval_secs > 0 {
        let refresher = pool.clone();
        let period = std::time::Duration::from_secs(args.refresh_interval_secs);
        info!("Refreshing snapshot every {}s", args.refresh_interval_secs);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            ticker.tick().await; // the first tick fires immediately
            loop {
                ticker.tick().await;
                // A failed refresh is not fatal: the snapshot simply stays
                // where it was, and a write will force a replay anyway.
                if let Err(e) = refresher.catch_up().await {
                    tracing::warn!(error = %e, "snapshot refresh failed");
                }
            }
        });
    }

    // ── 8. App state ──────────────────────────────────────────────────────────
    let state = AppState::new(
        pool,
        authorizer,
        build_credential_vendor(args.enable_aws_credentials),
        metastore_id,
        args.oidc_issuer.is_some(),
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

    // Nothing to flush at exit: every commit is already durable in the object
    // store by the time it is acknowledged. The shutdown channel that used to
    // live here existed only to push the SQLite file back to S3.
    axum::serve(listener, app).await.context("Server error")?;
    Ok(())
}

// ── Log-structured store ────────────────────────────────────────────────────────

/// Open the metadata log at `s3://bucket/prefix`.
///
/// `AWS_ENDPOINT_URL` redirects to MinIO, and forces path-style addressing:
/// virtual-host style would resolve `bucket.minio.svc` as a hostname, which
/// does not exist in-cluster.
async fn open_log_store(storage_root: &str) -> anyhow::Result<AnyPool> {
    use std::sync::Arc;

    let rest = storage_root.strip_prefix("s3://").ok_or_else(|| {
        anyhow::anyhow!("--storage-root must be s3://bucket[/prefix], got {storage_root:?}")
    })?;
    let (bucket, prefix) = match rest.split_once('/') {
        Some((b, p)) => (b, p),
        None => (rest, ""),
    };
    if bucket.is_empty() {
        anyhow::bail!("--storage-root has no bucket: {storage_root:?}");
    }

    let sdk = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let mut builder = aws_sdk_s3::config::Builder::from(&sdk);
    if let Ok(endpoint) = std::env::var("AWS_ENDPOINT_URL") {
        if !endpoint.is_empty() {
            builder = builder.endpoint_url(endpoint).force_path_style(true);
        }
    }
    let client = aws_sdk_s3::Client::from_conf(builder.build());

    info!("Metadata log: s3://{bucket}/{prefix}");
    let log = Arc::new(uc_db::store::s3::S3Log::new(client, bucket, prefix));
    uc_db::store::Store::open(log)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}






// ── OIDC JWKS discovery ───────────────────────────────────────────────────────

async fn fetch_oidc_jwks(issuer: &str) -> anyhow::Result<JwkSet> {
    let issuer = issuer.trim_end_matches('/');
    let discovery_url = format!("{issuer}/.well-known/openid-configuration");

    // In-cluster: load the k8s CA cert and SA bearer token from the automounted SA volume.
    // - CA cert: lets reqwest/rustls verify the k3s API server's self-signed cert.
    // - Bearer token: k3s requires auth on /.well-known/openid-configuration (returns 401 otherwise).
    let mut builder = reqwest::Client::builder();
    if let Ok(pem) = tokio::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt").await {
        if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder.build().context("Failed to build HTTP client")?;

    let sa_token = tokio::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/token")
        .await
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

    // The response is whatever the issuer returned; `.get` rather than `[]` so a
    // non-object body is an error instead of a panic.
    let jwks_uri = discovery
        .get("jwks_uri")
        .and_then(serde_json::Value::as_str)
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
