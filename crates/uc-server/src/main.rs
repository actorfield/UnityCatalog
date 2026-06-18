use anyhow::Context;
use axum::Router;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uc_api::{
    catalog_api, control_api, delta_api,
    middleware::auth_middleware,
    state::AppState,
};
use uc_auth::{AllowingAuthorizer, JwkSet, JwtConfig, KeyManager, OidcConfig, UcAuthorizer};
use uc_credentials::CloudCredentialVendor;
use uc_db::{pool::run_migrations, AnyPool, repos::{MetastoreRepo, UserRepo}};
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

    #[arg(long, default_value = "info")]
    log_level: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| format!("uc_server={l},uc_api={l},uc_db={l}", l = args.log_level).into()))
        .with(tracing_subscriber::fmt::layer().with_ansi(false))
        .init();

    info!("Starting Unity Catalog server on port {}", args.port);
    info!("Config dir: {}", args.config_dir.display());
    info!("Database:   {}", args.database_url);
    info!("Auth:       {}", if args.no_auth { "disabled" } else { "enabled" });

    // ── 1. RSA key initialization ─────────────────────────────────────────────
    let key_manager = KeyManager::load_or_generate(&args.config_dir)
        .context("Failed to initialize RSA keys")?;
    let jwt_config = JwtConfig::from_der(
        &key_manager.private_key_der,
        &key_manager.public_key_der,
        key_manager.key_id.clone(),
    ).context("Failed to create JWT config")?;

    // ── 2. Database pool + migrations ─────────────────────────────────────────
    std::fs::create_dir_all("./etc/db").ok();
    let pool = AnyPool::connect(&args.database_url)
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
            UserRepo::create(&pool, admin_id, admin_email, Some(admin_email), None, "ENABLED", now).await
                .context("Failed to create admin user")?;
            authorizer.grant(admin_id, metastore_id, uc_types::Privilege::Owner).await
                .context("Failed to grant admin OWNER on metastore")?;
            info!("Created admin user: {}", admin_email);
        }
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
            let jwks = fetch_oidc_jwks(issuer).await
                .context("Failed to fetch OIDC JWKS")?;
            info!("OIDC auth enabled, issuer: {}", issuer);
            Some(Arc::new(OidcConfig { issuer: issuer.clone(), jwks }))
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
        CloudCredentialVendor::new(),
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
        .route("/", axum::routing::get(|| async { "Hello, Unity Catalog!" }))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

    // ── 10. Bind and serve ────────────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to port")?;

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}

// ── OIDC JWKS discovery ───────────────────────────────────────────────────────

async fn fetch_oidc_jwks(issuer: &str) -> anyhow::Result<JwkSet> {
    let issuer = issuer.trim_end_matches('/');
    let discovery_url = format!("{issuer}/.well-known/openid-configuration");

    // In-cluster: load the k8s CA cert from the automounted SA volume so reqwest/rustls
    // can verify the k3s API server's self-signed cert without --oidc-insecure.
    let mut builder = reqwest::Client::builder();
    if let Ok(pem) = std::fs::read("/var/run/secrets/kubernetes.io/serviceaccount/ca.crt") {
        if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let client = builder.build().context("Failed to build HTTP client")?;

    let discovery: serde_json::Value = client
        .get(&discovery_url)
        .send()
        .await
        .context("OIDC discovery request failed")?
        .json()
        .await
        .context("OIDC discovery response not valid JSON")?;
    let jwks_uri = discovery["jwks_uri"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("OIDC discovery response missing 'jwks_uri'"))?;
    let jwks: JwkSet = client
        .get(jwks_uri)
        .send()
        .await
        .context("JWKS fetch failed")?
        .json()
        .await
        .context("JWKS response not valid JSON")?;
    Ok(jwks)
}
