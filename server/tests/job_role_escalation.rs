//! Anti-escalada de privilégio via `POST /sync/job-roles` (§11).
//!
//! Uma Função (RBAC) define o conjunto de permissões que vira o JWT no
//! próximo login. A rota REST de Funções sempre exigiu `require_can_grant`
//! (não conceder o que não se possui); o caminho de SYNC ficou sem esse gate,
//! e um gerente com `collaborators.edit` reescrevia a própria Função com o
//! conjunto total — no relogin, `finance.*`, `cash.*`, etc.
//!
//! Requer `TEST_DATABASE_URL` — sem ela o teste é PULADO.

mod comum;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use letaf_server::bootstrap::build_state;
use letaf_server::config::AppConfig;
use letaf_server::jwt::{create_token, ROLE_EMPLOYEE};
use letaf_server::routes::create_routes;

const JWT_SECRET: &str = "segredo-de-teste-escalada-nao-usar-em-prod";

async fn resposta_status(
    app: &axum::Router,
    token: &str,
    corpo: serde_json::Value,
) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/sync/job-roles")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(corpo.to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn gerente_nao_concede_permissao_que_nao_possui() {
    let Some((pool, schema)) = comum::banco_de_teste("escalada").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    unsafe { std::env::set_var("JWT_SECRET", JWT_SECRET) };
    let company_id = Uuid::new_v4();
    let jr_id = Uuid::new_v4();

    // Empresa + uma Função "Gerente" que só gere colaboradores.
    sqlx::query(
        "INSERT INTO companies (id, name, subdomain, created_at, updated_at, synced)
         VALUES ($1, 'Loja', $2, now(), now(), true)",
    )
    .bind(company_id)
    .bind(format!("t{}", company_id.simple()))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO job_roles (id, company_id, name, permissions, created_at, updated_at, synced)
         VALUES ($1, $2, 'Gerente', $3, now(), now(), true)",
    )
    .bind(jr_id)
    .bind(company_id)
    .bind(r#"["collaborators.view","collaborators.edit"]"#)
    .execute(&pool)
    .await
    .unwrap();

    // Usuário REAL do gerente: o middleware revoga (401) token cujo `sub` não
    // existe ou cuja `token_version` não bate — então ele precisa existir com
    // tv=0.
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, company_id, email, password_hash, name, role,
                            job_role_id, token_version, created_at, updated_at, synced)
         VALUES ($1, $2, $3, 'x', 'Gerente', 'employee', $4, 0, now(), now(), true)",
    )
    .bind(user_id)
    .bind(company_id)
    .bind(format!("g{}@t.com", user_id.simple()))
    .bind(jr_id)
    .execute(&pool)
    .await
    .unwrap();

    let state = build_state(pool.clone(), AppConfig::from_env());
    let app = create_routes().with_state(state);

    // Token do gerente: exatamente as permissões da Função dele.
    let token = create_token(
        user_id,
        company_id,
        ROLE_EMPLOYEE,
        vec!["collaborators.view".into(), "collaborators.edit".into()],
        0,
        JWT_SECRET,
        1,
    )
    .unwrap();

    // Ataque: reescrever a própria Função com o conjunto total.
    let escalada = json!({
        "id": jr_id,
        "company_id": company_id,
        "name": "Gerente",
        "permissions": ["collaborators.view", "collaborators.edit",
                        "finance.view", "finance.edit", "cash.view", "cash.edit"],
        "created_at": "2026-01-01T00:00:00",
        "updated_at": "2999-01-01T00:00:00",
        "deleted_at": null,
        "synced": false
    });
    let st = resposta_status(&app, &token, escalada).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "conceder finance/cash sem possuí-las tem que dar 403, não {st}"
    );

    // O banco NÃO pode ter sido alterado — a Função segue só com colaboradores.
    // `permissions` é TEXT (JSON serializado como string), não JSONB.
    let perms: String = sqlx::query_scalar("SELECT permissions FROM job_roles WHERE id = $1")
        .bind(jr_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let lista: Vec<String> = serde_json::from_str(&perms).unwrap();
    assert!(
        !lista.iter().any(|p| p.starts_with("finance") || p.starts_with("cash")),
        "a Função foi escalada apesar do 403: {lista:?}"
    );

    // Contraprova: conceder só o que já se possui é aceito.
    let ok = json!({
        "id": jr_id,
        "company_id": company_id,
        "name": "Gerente de equipe",
        "permissions": ["collaborators.view", "collaborators.edit"],
        "created_at": "2026-01-01T00:00:00",
        "updated_at": "2999-01-02T00:00:00",
        "deleted_at": null,
        "synced": false
    });
    assert_eq!(resposta_status(&app, &token, ok).await, StatusCode::OK);

    comum::derrubar(pool, &schema).await;
}
