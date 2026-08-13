//! O servidor renumera pedido em colisão de `number` no sync, em vez de
//! envenenar a fila (§7.6, §7.7).
//!
//! O número do pedido é `MAX(number)+1` LOCAL de cada terminal. Dois terminais
//! offline geram o MESMO número para vendas DIFERENTES (ids distintos). Sem
//! tratamento, o segundo push viola o `UNIQUE(company_id, number)` e re-tenta
//! para sempre — a venda nunca chega ao servidor. A autoridade é o servidor:
//! ao receber um id NOVO cujo número já pertence a OUTRO pedido, ele renumera
//! para o próximo livre e bumpa `updated_at`, e o desktop reconverte no pull.
//!
//! Requer `TEST_DATABASE_URL` — sem ela o teste é PULADO.

mod comum;

use rust_decimal_macros::dec;
use uuid::Uuid;

use letaf_core::order::model::{DeliveryType, Order};
use letaf_core::order::repository::OrderRepository;
use letaf_server::repository::order::PgOrderRepository;

/// Pedido sem itens (o foco é a numeração, não o estoque), com número fixo.
fn pedido(company_id: Uuid, number: i64) -> Order {
    let mut o = Order::new(company_id, Uuid::nil(), dec!(10), DeliveryType::Pickup, None);
    o.number = number;
    o
}

async fn numero_de(pool: &sqlx::PgPool, id: Uuid) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT number FROM orders WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn colisao_de_numero_renumera_em_vez_de_envenenar() {
    let Some((pool, schema)) = comum::banco_de_teste("order_number").await else {
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

    let repo = PgOrderRepository::new(pool.clone());

    // Terminal A: pedido nº 1.
    let a = pedido(company_id, 1);
    repo.sync_upsert(&a).await.expect("push do pedido A");
    assert_eq!(numero_de(&pool, a.base.id).await, 1);

    // Terminal B: venda DIFERENTE (outro id) que também recebeu o nº 1.
    // Sem renumeração, este push falharia no UNIQUE(company_id, number).
    let b = pedido(company_id, 1);
    assert_ne!(a.base.id, b.base.id);
    repo.sync_upsert(&b)
        .await
        .expect("push do pedido B NÃO pode falhar por colisão de número");

    // A mantém o nº 1; B foi renumerado para o próximo livre (2). Duas vendas
    // preservadas, nenhuma perdida.
    assert_eq!(numero_de(&pool, a.base.id).await, 1, "A preserva o número");
    assert_eq!(numero_de(&pool, b.base.id).await, 2, "B renumerado p/ MAX+1");

    // Re-push do MESMO pedido B (id já existente) NÃO pode renumerar de novo:
    // a renumeração só vale para id NOVO. O número tem que ficar estável.
    let mut b2 = b.clone();
    b2.number = 2;
    b2.base.updated_at = chrono::Utc::now().naive_utc();
    repo.sync_upsert(&b2).await.expect("re-push do B");
    assert_eq!(numero_de(&pool, b.base.id).await, 2, "id existente não renumera");

    comum::derrubar(pool, &schema).await;
}
