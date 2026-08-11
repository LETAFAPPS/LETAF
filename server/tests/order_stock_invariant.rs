//! O servidor impõe o invariante de estoque do pedido (§7.7, §11).
//!
//! O delta de estoque de uma edição era calculado por cada terminal contra a
//! PRÓPRIA cópia do pedido, mas só uma versão sobrevive ao last-write-wins.
//! Com A editando 2→3 (delta −1) e B editando 2→5 (delta −3) na mesma janela,
//! os DOIS deltas eram aplicados (−4) enquanto o pedido final dizia 5 —
//! estoque e pedido ficavam permanentemente descasados, convergentes e
//! errados, sem erro nenhum em lugar algum.
//!
//! O servidor é a autoridade: depois de cada upsert vencedor ele reconcilia o
//! ledger, garantindo que a soma dos deltas do pedido para cada produto valha
//! exatamente `-quantidade` (ou `0` se o pedido foi cancelado).
//!
//! Requer `TEST_DATABASE_URL` — sem ela os testes são pulados.

mod comum;

use rust_decimal_macros::dec;
use sqlx::PgPool;
use uuid::Uuid;

use letaf_core::order::model::{DeliveryType, Order, OrderItem, OrderStatus};
use letaf_core::order::repository::OrderRepository;
use letaf_server::repository::order::PgOrderRepository;

/// Empresa + produto com estoque conhecido. Devolve `(company_id, product_id)`.
async fn cenario(pool: &PgPool, estoque: f64) -> (Uuid, Uuid) {
    let company_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO companies (id, name, subdomain, created_at, updated_at, synced)
         VALUES ($1, 'Loja Teste', $2, now(), now(), true)",
    )
    .bind(company_id)
    .bind(format!("t{}", company_id.simple()))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO products (id, company_id, name, price, stock_quantity, unlimited_stock,
                               unit, created_at, updated_at, synced, content_updated_at)
         VALUES ($1, $2, 'Coca', 5.00, $3, false, 'un', now(), now(), true, now())",
    )
    .bind(product_id)
    .bind(company_id)
    .bind(estoque)
    .execute(pool)
    .await
    .unwrap();
    (company_id, product_id)
}

fn pedido(company_id: Uuid, product_id: Uuid, quantidade: f64) -> Order {
    let mut o = Order::new(company_id, Uuid::nil(), dec!(10), DeliveryType::Pickup, None);
    o.number = 1;
    o.items = vec![OrderItem::new(
        company_id,
        o.base.id,
        product_id,
        "Coca".into(),
        quantidade,
        dec!(5),
        None,
        None,
    )];
    o
}

async fn soma_do_ledger(pool: &PgPool, order_id: Uuid, product_id: Uuid) -> f64 {
    sqlx::query_scalar::<_, f64>(
        "SELECT COALESCE(SUM(delta), 0)::float8 FROM stock_movements
          WHERE order_id = $1 AND product_id = $2",
    )
    .bind(order_id)
    .bind(product_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn estoque_de(pool: &PgPool, product_id: Uuid) -> f64 {
    sqlx::query_scalar::<_, f64>("SELECT stock_quantity FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Grava um movimento de edição direto no ledger, como faria um terminal.
async fn movimento_de_terminal(pool: &PgPool, company_id: Uuid, order_id: Uuid, product_id: Uuid, delta: f64) {
    sqlx::query(
        "INSERT INTO stock_movements
            (id, company_id, product_id, delta, reason, order_id, created_at, updated_at, synced)
         VALUES ($1, $2, $3, $4, 'edit', $5, now(), now(), true)",
    )
    .bind(Uuid::new_v4())
    .bind(company_id)
    .bind(product_id)
    .bind(delta)
    .bind(order_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET stock_quantity = stock_quantity + $1 WHERE id = $2")
        .bind(delta)
        .bind(product_id)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn edicao_concorrente_converge_estoque_com_o_pedido_vencedor() {
    let Some((pool, schema)) = comum::banco_de_teste("estoque_edicao").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (company_id, product_id) = cenario(&pool, 10.0).await;
    let repo = PgOrderRepository::new(pool.clone());

    // Pedido original: 2 unidades, estoque 10 → 8.
    let mut o = pedido(company_id, product_id, 2.0);
    repo.create_atomic(&o, &[(product_id, 2.0)], None).await.unwrap();
    assert_eq!(estoque_de(&pool, product_id).await, 8.0);

    // Terminal A edita 2→3 (delta −1) e terminal B edita 2→5 (delta −3).
    // Os dois calcularam contra a mesma base e os dois movimentos sobem.
    movimento_de_terminal(&pool, company_id, o.base.id, product_id, -1.0).await;
    movimento_de_terminal(&pool, company_id, o.base.id, product_id, -3.0).await;
    assert_eq!(soma_do_ledger(&pool, o.base.id, product_id).await, -6.0);
    assert_eq!(estoque_de(&pool, product_id).await, 4.0);

    // O push de B vence o LWW: o pedido passa a valer 5 unidades.
    o.items[0].quantity = 5.0;
    o.base.updated_at = chrono::Utc::now().naive_utc();
    let venceu = repo.sync_upsert(&o).await.unwrap();
    assert!(venceu, "o push mais novo tem que vencer o LWW");

    // Invariante: o ledger do pedido vale exatamente −5, e o estoque
    // corresponde (10 − 5).
    assert_eq!(
        soma_do_ledger(&pool, o.base.id, product_id).await,
        -5.0,
        "a soma dos deltas tem que bater com o pedido vencedor"
    );
    assert_eq!(
        estoque_de(&pool, product_id).await,
        5.0,
        "o estoque tem que refletir as 5 unidades do pedido, não as 6 dos deltas somados"
    );

    comum::derrubar(pool, &schema).await;
}

#[tokio::test]
async fn reconciliacao_e_idempotente_quando_o_ledger_ja_bate() {
    let Some((pool, schema)) = comum::banco_de_teste("estoque_idem").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (company_id, product_id) = cenario(&pool, 10.0).await;
    let repo = PgOrderRepository::new(pool.clone());

    let mut o = pedido(company_id, product_id, 2.0);
    repo.create_atomic(&o, &[(product_id, 2.0)], None).await.unwrap();

    // Reenvios sucessivos sem mudança de quantidade não podem gerar
    // movimento nenhum — a diferença é zero.
    for _ in 0..3 {
        o.base.updated_at = chrono::Utc::now().naive_utc();
        repo.sync_upsert(&o).await.unwrap();
    }

    assert_eq!(soma_do_ledger(&pool, o.base.id, product_id).await, -2.0);
    assert_eq!(estoque_de(&pool, product_id).await, 8.0);
    let movimentos: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stock_movements WHERE order_id = $1")
        .bind(o.base.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(movimentos, 1, "nenhum movimento de compensação deve ser criado");

    comum::derrubar(pool, &schema).await;
}

/// Pedido cancelado devolve o estoque: o alvo do invariante passa a ser zero.
#[tokio::test]
async fn cancelamento_zera_o_ledger_do_pedido() {
    let Some((pool, schema)) = comum::banco_de_teste("estoque_cancel").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (company_id, product_id) = cenario(&pool, 10.0).await;
    let repo = PgOrderRepository::new(pool.clone());

    let mut o = pedido(company_id, product_id, 2.0);
    repo.create_atomic(&o, &[(product_id, 2.0)], None).await.unwrap();
    assert_eq!(estoque_de(&pool, product_id).await, 8.0);

    o.status = OrderStatus::Cancelled;
    o.base.updated_at = chrono::Utc::now().naive_utc();
    repo.sync_upsert(&o).await.unwrap();

    assert_eq!(
        soma_do_ledger(&pool, o.base.id, product_id).await,
        0.0,
        "pedido cancelado não pode deixar baixa no ledger"
    );
    assert_eq!(estoque_de(&pool, product_id).await, 10.0, "estoque volta ao original");

    comum::derrubar(pool, &schema).await;
}

/// Pedido histórico SEM movimento nenhum não pode ganhar uma baixa retroativa
/// só por ser reenviado.
#[tokio::test]
async fn pedido_sem_ledger_nao_ganha_baixa_retroativa() {
    let Some((pool, schema)) = comum::banco_de_teste("estoque_hist").await else {
        eprintln!("TEST_DATABASE_URL ausente — teste pulado");
        return;
    };
    let (company_id, product_id) = cenario(&pool, 10.0).await;
    let repo = PgOrderRepository::new(pool.clone());

    // Entra pelo caminho de sync (sem `create_atomic`), então não há ledger.
    let mut o = pedido(company_id, product_id, 2.0);
    repo.sync_upsert(&o).await.unwrap();
    o.base.updated_at = chrono::Utc::now().naive_utc();
    repo.sync_upsert(&o).await.unwrap();

    assert_eq!(soma_do_ledger(&pool, o.base.id, product_id).await, 0.0);
    assert_eq!(
        estoque_de(&pool, product_id).await,
        10.0,
        "sem ledger prévio o pedido não é considerado controlado por estoque"
    );

    comum::derrubar(pool, &schema).await;
}
