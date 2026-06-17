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
use uc_auth::{AllowingAuthorizer, JwtConfig, KeyManager, UcAuthorizer};
use uc_credentials::CloudCredentialVendor;
use uc_db::{pool::run_migrations, AnyPool, repos::{MetastoreRepo, UserRepo}};
use uuid::Uuid;

// ── CLI args ──────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "uc-server", about = "Unity Catalog server (Rust)")]
struct Args {
    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Path to configuration directory (contains server.properties, certs, DB)
    #[arg(long, default_value = "./etc/conf")]
    config_dir: PathBuf,

    /// Database URL (sqlite:./etc/db/uc.db or postgres://...)
    #[arg(long, default_value = "sqlite:./etc/db/uc.db?mode=rwc")]
    database_url: String,

    /// Disable authorization (allow all requests)
    #[arg(long, default_value_t = false)]
    no_auth: bool,

    /// Log level: error, warn, info, debug, trace (or RUST_LOG env var)
    #[arg(long, default_value = "info")]
    log_level: String,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // #1407: log level configurable via --log-level or RUST_LOG env var
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| format!("uc_server={l},uc_api={l},uc_db={l}", l = args.log_level).into()))
        .with(tracing_subscriber::fmt::layer())
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

    // Write service token — uses the admin email as sub so auth-enabled handlers
    // can resolve it to the admin user who has OWNER on the metastore
    let admin_email = "admin@unitycatalog.io";
    let service_claims = uc_auth::jwt::UcClaims::new_access(admin_email);
    let service_token = uc_auth::jwt::encode_token(&jwt_config, &service_claims)
        .context("Failed to create service token")?;
    std::fs::write(args.config_dir.join("token.txt"), &service_token)
        .context("Failed to write service token")?;

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
    if !args.no_auth {
        let admin_email = "admin@unitycatalog.io";
        if UserRepo::get_by_email(&pool, admin_email).await?.is_none() {
            let admin_id = Uuid::new_v4();
            let now = chrono::Utc::now().timestamp_millis();
            UserRepo::create(&pool, admin_id, admin_email, Some(admin_email), None, "ENABLED", now).await
                .context("Failed to create admin user")?;
            authorizer.grant(admin_id, metastore_id, uc_types::Privilege::Owner).await
                .context("Failed to grant admin OWNER on metastore")?;
            info!("Created admin user: {}", admin_email);
        }
    }

    // ── 6. App state ──────────────────────────────────────────────────────────
    let state = AppState::new(
        pool,
        authorizer,
        CloudCredentialVendor::new(),
        jwt_config,
        metastore_id,
        !args.no_auth,
        args.config_dir.clone(),
    );

    // ── 7. Router assembly ────────────────────────────────────────────────────
    let app = Router::new()
        .merge(catalog_api::router(state.clone()))
        .merge(control_api::router(state.clone()))
        .merge(delta_api::router(state.clone()))
        .route("/", axum::routing::get(|| async { "Hello, Unity Catalog!" }))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

    // ── 8. Bind and serve ─────────────────────────────────────────────────────
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
