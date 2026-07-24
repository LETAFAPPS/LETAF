//! Testes de INTEGRAÇÃO do gate das rotas `/admin/*`.
//!
//! Os testes unitários em `middleware::auth` cobrem a decisão de
//! autorização isoladamente. Aqui exercitamos o caminho completo —
//! roteamento do axum, extração do JWT e o handler — contra um Postgres
//! real, que é onde um erro de fiação (rota registrada sem o extrator,
//! guard chamado tarde demais) apareceria.
//!
//! Requer `TEST_DATABASE_URL`. Sem ela os testes são PULADOS (não falham),
//! para `cargo test` seguir funcionando em máquina sem banco — o CI provê
//! um container de Postgres.
//!
//! O app é montado com o MESMO `build_state` da produção (§8 — sem
//! duplicar fiação, sem risco de o teste divergir do app real).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt; // oneshot
use uuid::Uuid;

use letaf_server::bootstrap::build_state;
use letaf_server::config::AppConfig;
use letaf_server::jwt::{create_token, ROLE_ADMIN, ROLE_CUSTOMER, ROLE_EMPLOYEE, ROLE_SUPER_ADMIN};
use letaf_server::routes::create_routes;

const JWT_SECRET: &str = "segredo-de-teste-nao-usar-em-producao";

/// Monta o router real contra o banco de teste. `None` quando não há
/// `TEST_DATABASE_URL` — o chamador pula o teste.
async fn app() -> Option<(axum::Router, sqlx::PgPool)> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    // O AppConfig sai do ambiente; fixamos o segredo do JWT para poder
    // forjar tokens de cada papel nos testes.
    unsafe { std::env::set_var("JWT_SECRET", JWT_SECRET) };
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("conectar no TEST_DATABASE_URL");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("aplicar migrações no banco de teste");
    let state = build_state(pool.clone(), AppConfig::from_env());
    Some((create_routes().with_state(state), pool))
}

/// Token válido para um papel qualquer (empresa fictícia).
fn token(role: &str) -> String {
    create_token(
        Uuid::new_v4(),
        Uuid::new_v4(),
        role,
        Vec::new(),
        0,
        JWT_SECRET,
        1,
    )
    .expect("gerar token")
}

/// Cria uma empresa + um operador com o papel pedido e devolve um token
/// VÁLIDO para ele.
///
/// O usuário precisa EXISTIR: o extrator `AuthClaims` valida
/// `token_version` no banco para admin/employee — com um `sub` inventado a
/// requisição morre em 401 ANTES do gate, e o teste passaria pelo motivo
/// errado, sem acusar uma falha no guard.
///
/// A fixture é criada pelo próprio teste (não depende de dado pré-existente,
/// o banco do CI nasce vazio) e removida no fim.
async fn operador_fixture(pool: &sqlx::PgPool, role: &str) -> (Uuid, Uuid, String) {
    let company_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    sqlx::query("INSERT INTO companies (id, name, subdomain) VALUES ($1, $2, $3)")
        .bind(company_id)
        .bind(format!("Teste {role}"))
        .bind(format!("teste-{}", &user_id.to_string()[..8]))
        .execute(pool)
        .await
        .expect("criar empresa de teste");
    sqlx::query(
        "INSERT INTO users (id, company_id, email, password_hash, name, role)
         VALUES ($1, $2, $3, 'x', $4, $5)",
    )
    .bind(user_id)
    .bind(company_id)
    .bind(format!("{}@teste.local", &user_id.to_string()[..8]))
    .bind(format!("Operador {role}"))
    .bind(role)
    .execute(pool)
    .await
    .expect("criar usuário de teste");
    let token = create_token(user_id, company_id, role, Vec::new(), 0, JWT_SECRET, 1)
        .expect("gerar token");
    (company_id, user_id, token)
}

/// Remove a fixture (a trilha de auditoria e demais tabelas não são
/// tocadas por estes testes, que só fazem GET).
async fn limpar_fixture(pool: &sqlx::PgPool, company_id: Uuid, user_id: Uuid) {
    let _ = sqlx::query("DELETE FROM users WHERE id = $1").bind(user_id).execute(pool).await;
    let _ = sqlx::query("DELETE FROM companies WHERE id = $1").bind(company_id).execute(pool).await;
}

/// GET numa rota, opcionalmente autenticado.
async fn get(app: &axum::Router, path: &str, bearer: Option<&str>) -> StatusCode {
    let mut req = Request::builder().uri(path).method("GET");
    if let Some(t) = bearer {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("resposta")
        .status()
}

/// Todas as rotas de LEITURA do painel, para varrer o gate de uma vez.
const ROTAS_GET: &[&str] = &[
    "/admin/overview",
    "/admin/companies",
    "/admin/subscriptions",
    "/admin/admins",
    "/admin/plans",
    "/admin/audit",
];

#[tokio::test]
async fn sem_token_o_painel_recusa() {
    let Some((app, _pool)) = app().await else { return };
    for rota in ROTAS_GET {
        let s = get(&app, rota, None).await;
        assert!(
            s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN,
            "{rota} sem token devolveu {s}"
        );
    }
}

#[tokio::test]
async fn operador_comum_nao_acessa_o_painel() {
    let Some((app, pool)) = app().await else { return };
    // Admin de loja, funcionário e cliente final NÃO são super admin.
    // Para admin/employee usamos um usuário REAL (ver token_operador_real);
    // o cliente final não passa pela checagem de token_version.
    let mut criados = Vec::new();
    let mut tokens: Vec<(&str, String)> = Vec::new();
    for role in [ROLE_ADMIN, ROLE_EMPLOYEE] {
        let (cid, uid, t) = operador_fixture(&pool, role).await;
        criados.push((cid, uid));
        tokens.push((role, t));
    }
    // Cliente final não passa pela checagem de token_version.
    tokens.push((ROLE_CUSTOMER, token(ROLE_CUSTOMER)));

    for (role, t) in &tokens {
        for rota in ROTAS_GET {
            let s = get(&app, rota, Some(t)).await;
            assert!(
                s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN,
                "{rota} com role {role} devolveu {s} (deveria recusar)"
            );
        }
    }
    for (cid, uid) in criados {
        limpar_fixture(&pool, cid, uid).await;
    }
}

#[tokio::test]
async fn super_admin_passa_pelo_gate() {
    let Some((app, _pool)) = app().await else { return };
    let t = token(ROLE_SUPER_ADMIN);
    for rota in ROTAS_GET {
        let s = get(&app, rota, Some(&t)).await;
        // O gate não pode recusar. O conteúdo pode variar (200) — o que
        // importa aqui é NÃO ser 401/403.
        assert!(
            s != StatusCode::UNAUTHORIZED && s != StatusCode::FORBIDDEN,
            "{rota} recusou um super admin ({s})"
        );
    }
}

#[tokio::test]
async fn token_assinado_com_outro_segredo_e_recusado() {
    let Some((app, _pool)) = app().await else { return };
    let forjado = create_token(
        Uuid::new_v4(),
        Uuid::new_v4(),
        ROLE_SUPER_ADMIN,
        Vec::new(),
        0,
        "segredo-errado",
        1,
    )
    .expect("gerar token");
    let s = get(&app, "/admin/overview", Some(&forjado)).await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "token com assinatura inválida não pode ser aceito"
    );
}
