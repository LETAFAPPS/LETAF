//! Revogação da sessão de IMPERSONATION quando o super admin é desativado.
//!
//! O token de impersonation é emitido como o dono da loja (`role=admin`), mas
//! carimbado com o super admin que o abriu (`imp`). Sem a revogação, um super
//! admin desativado mantinha acesso à loja pela sessão já aberta — o dono
//! continua ativo, então a checagem de `token_version` do dono passava. Este
//! teste tranca a semântica: desativar o super admin derruba a sessão no
//! próximo request.
//!
//! Requer `TEST_DATABASE_URL` — sem ela é PULADO.

mod comum;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use letaf_server::bootstrap::build_state;
use letaf_server::config::AppConfig;
use letaf_server::jwt::create_impersonation_token;
use letaf_server::routes::create_routes;

const JWT_SECRET: &str = "segredo-de-teste-impersonation-nao-usar";

async fn get(app: &axum::Router, path: &str, bearer: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method("GET")
                .header("authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("resposta")
        .status()
}

#[tokio::test]
async fn desativar_super_admin_revoga_a_impersonation() {
    let Some((pool, schema)) = comum::banco_de_teste("imp_revoke").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    unsafe { std::env::set_var("JWT_SECRET", JWT_SECRET) };

    // Loja alvo + o dono (admin) que a sessão de impersonation assume.
    let company_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    sqlx::query("INSERT INTO companies (id, name, subdomain) VALUES ($1, 'Loja', $2)")
        .bind(company_id)
        .bind(format!("t{}", company_id.simple()))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO users (id, company_id, email, password_hash, name, role, token_version)
         VALUES ($1, $2, $3, 'x', 'Dono', 'admin', 0)",
    )
    .bind(owner_id)
    .bind(company_id)
    .bind(format!("dono-{}@t.local", owner_id.simple()))
    .execute(&pool)
    .await
    .unwrap();

    // O super admin que impersona, ATIVO em admin_user_roles.
    let super_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO admin_user_roles (user_id, role_id, active) VALUES ($1, $2, true)",
    )
    .bind(super_id)
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .unwrap();

    let state = build_state(pool.clone(), AppConfig::from_env());
    let app = create_routes().with_state(state);

    // Token de impersonation: sessão do dono, carimbada com o super admin.
    let token = create_impersonation_token(
        owner_id,
        company_id,
        "admin",
        Vec::new(),
        0,
        super_id,
        JWT_SECRET,
        1,
    )
    .unwrap();

    // Rota de operador do tenant (aceita admin, valida token_version do dono).
    let rota = "/sync/pull/products?since=1970-01-01T00:00:00";

    // Super admin ATIVO → a sessão funciona (200).
    assert_eq!(
        get(&app, rota, &token).await,
        StatusCode::OK,
        "com o super admin ativo, a impersonation deve funcionar"
    );

    // Desativa o super admin.
    sqlx::query("UPDATE admin_user_roles SET active = false WHERE user_id = $1")
        .bind(super_id)
        .execute(&pool)
        .await
        .unwrap();

    // MESMO token → agora rejeitado, mesmo o dono seguindo ativo.
    assert_eq!(
        get(&app, rota, &token).await,
        StatusCode::UNAUTHORIZED,
        "desativar o super admin tem que revogar a sessão de impersonation"
    );

    comum::derrubar(pool, &schema).await;
}
