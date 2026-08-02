//! Upsert de produto pelo sync, exercitado contra o Postgres real.
//!
//! Regressão que motivou o teste: a migration do `content_updated_at` deixou
//! DUAS atribuições de `updated_at` no mesmo `DO UPDATE SET` (`= EXCLUDED` e
//! `= GREATEST(...)`). O Postgres recusa isso com "multiple assignments to
//! same column", mas só na EXECUÇÃO de um upsert sobre um produto EXISTENTE —
//! o `sync_parity` (que lê o texto do SQL) e os testes de `create` não
//! exercitavam esse caminho, então o push de todo produto já cadastrado
//! falhava com 500 sem nenhum teste acusar.
//!
//! Requer `TEST_DATABASE_URL` — sem ela o teste é PULADO.

mod comum;

use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::product::model::{BalanceMode, Product};
use letaf_core::product::repository::ProductRepository;
use letaf_server::repository::product::PgProductRepository;

fn produto(company_id: Uuid, nome: &str) -> Product {
    Product::new(
        company_id,
        nome.into(),
        None,
        None,
        None,
        Some(dec!(10)),
        None,
        5.0,
        0.0,
        false,
        None,
        "un".into(),
        BalanceMode::Weight,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

#[tokio::test]
async fn upsert_de_produto_existente_atualiza_sem_erro_de_sql() {
    let Some((pool, schema)) = comum::banco_de_teste("prod_upsert").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let company_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO companies (id, name, subdomain, created_at, updated_at, synced)
         VALUES ($1, 'Loja', $2, now(), now(), true)",
    )
    .bind(company_id)
    .bind(format!("t{}", company_id.simple()))
    .execute(&pool)
    .await
    .unwrap();

    let repo = PgProductRepository::new(pool.clone());

    // 1) Primeiro upsert: cria (INSERT do ON CONFLICT).
    let mut p = produto(company_id, "Coca");
    repo.sync_upsert(&p).await.expect("primeiro upsert (insert)");

    // 2) Segundo upsert do MESMO id: dispara o DO UPDATE SET — exatamente o
    // caminho que quebrava com "multiple assignments to same column".
    p.name = "Coca Zero".into();
    p.base.updated_at = chrono::Utc::now().naive_utc();
    repo.sync_upsert(&p)
        .await
        .expect("segundo upsert (update) NÃO pode falhar por SQL");

    let nome: String = sqlx::query_scalar("SELECT name FROM products WHERE id = $1")
        .bind(p.base.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(nome, "Coca Zero", "o update do upsert precisa ter aplicado");

    comum::derrubar(pool, &schema).await;
}
