//! Banco de teste para os testes de integração do servidor.
//!
//! Cada execução cria um SCHEMA próprio, roda as migrações dentro dele e o
//! derruba no fim. Schema (em vez de banco) porque criar banco exige
//! `CREATEDB`, que o papel da aplicação não tem — e não deve ter. Como
//! `search_path` é por conexão, o isolamento é total: dois testes rodando em
//! paralelo não se enxergam, e nada sobra no banco de desenvolvimento.
//!
//! Requer `TEST_DATABASE_URL` (o CI provê um Postgres em container). Sem ela
//! os testes são PULADOS, não falham — `cargo test` continua funcionando em
//! máquina sem banco.

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};

/// Banco pronto para uso: pool com `search_path` no schema exclusivo, com as
/// migrações já aplicadas. `None` quando não há `TEST_DATABASE_URL`.
///
/// Devolve o nome do schema para o [`derrubar`] do fim do teste.
pub async fn banco_de_teste(rotulo: &str) -> Option<(PgPool, String)> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    // Nome único por teste: o carimbo de tempo permite a faxina abaixo e o
    // sufixo aleatório evita colisão entre testes que o cargo roda em
    // paralelo na mesma máquina.
    let agora = chrono::Utc::now().timestamp();
    let schema = format!("teste_{agora}_{rotulo}_{}", uuid::Uuid::new_v4().simple());

    let raiz = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("conectar no TEST_DATABASE_URL");
    faxina(&raiz, agora).await;
    raiz.execute(format!("CREATE SCHEMA \"{schema}\"").as_str())
        .await
        .expect("criar schema de teste");
    raiz.close().await;

    let s = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |conn, _| {
            let s = s.clone();
            Box::pin(async move {
                // Toda conexão do pool enxerga só o schema do teste.
                conn.execute(format!("SET search_path TO \"{s}\"").as_str())
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("conectar no schema de teste");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("aplicar migrações no schema de teste");

    Some((pool, schema))
}

/// Remove schemas de execuções anteriores que ficaram para trás.
///
/// O `derrubar` do fim do teste não roda quando o teste entra em PÂNICO — e é
/// justamente aí, com o teste falhando, que alguém vai reexecutar várias
/// vezes. Sem esta faxina o banco de desenvolvimento acumulava um schema
/// completo (com todas as tabelas) por falha.
///
/// Só apaga o que tem mais de uma hora: nada em uso por uma execução paralela
/// é tão antigo, então a faxina nunca derruba schema de teste vivo.
async fn faxina(raiz: &PgPool, agora: i64) {
    let limite = agora - 3_600;
    let antigos: Vec<(String,)> = sqlx::query_as(
        "SELECT schema_name::text FROM information_schema.schemata
          WHERE schema_name LIKE 'teste\\_%'",
    )
    .fetch_all(raiz)
    .await
    .unwrap_or_default();
    for (nome,) in antigos {
        let velho = nome
            .strip_prefix("teste_")
            .and_then(|r| r.split('_').next())
            .and_then(|ts| ts.parse::<i64>().ok())
            // Sem carimbo legível: formato antigo, pode ir embora.
            .is_none_or(|ts| ts < limite);
        if velho {
            let _ = raiz
                .execute(format!("DROP SCHEMA \"{nome}\" CASCADE").as_str())
                .await;
        }
    }
}

/// Derruba o schema do teste. Chamar sempre no fim — inclusive quando o teste
/// falha, para não deixar lixo acumulando no banco.
pub async fn derrubar(pool: PgPool, schema: &str) {
    let _ = pool
        .execute(format!("DROP SCHEMA \"{schema}\" CASCADE").as_str())
        .await;
    pool.close().await;
}
