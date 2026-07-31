//! Guarda estrutural da PARIDADE DE SINCRONIZAÇÃO (AI_RULES.md §6, §7).
//!
//! O `sync_upsert` de cada entidade é escrito à mão nos dois bancos
//! (SQLite no desktop, PostgreSQL no servidor). Quando alguém adiciona um
//! campo ao modelo e esquece de incluí-lo no SQL, o campo simplesmente
//! NUNCA sincroniza — e o defeito é silencioso: nada quebra, nada aparece
//! no log, os dois lados só divergem para sempre. Foi exatamente o que
//! aconteceu com `companies.utc_offset_minutes`.
//!
//! Este teste lê os próprios fontes dos repositórios e cobra que cada
//! campo do modelo apareça no SQL de upsert dos DOIS lados.
//!
//! Exceções legítimas ficam em `FORA_DO_UPSERT`, com a justificativa
//! explícita — a exceção vira decisão consciente, não esquecimento.

/// (entidade, campo) que NÃO deve viajar, com o porquê.
const FORA_DO_UPSERT: &[(&str, &str, &str)] = &[
    // Autoridade do SERVIDOR: o desktop nunca sobrescreve.
    ("company", "active", "suspensão do tenant é da plataforma"),
    ("company", "subdomain", "chave de roteamento do tenant"),
    ("wallet", "balance", "saldo evolui só pelo ledger de movimentos"),
    ("product", "stock_quantity", "estoque evolui só pelo ledger"),
    // Campos de controle presentes em toda entidade, tratados à parte.
    ("*", "base", "BaseFields é expandido em colunas próprias"),
];

/// Modelos e os repositórios correspondentes nos dois bancos.
const ENTIDADES: &[(&str, &str, &str, &str)] = &[
    (
        "company",
        include_str!("../../core/src/company/model.rs"),
        include_str!("../../desktop/src/repository/company.rs"),
        include_str!("../src/repository/company.rs"),
    ),
    (
        "customer",
        include_str!("../../core/src/customer/model.rs"),
        include_str!("../../desktop/src/repository/customer.rs"),
        include_str!("../src/repository/customer.rs"),
    ),
    (
        "product",
        include_str!("../../core/src/product/model.rs"),
        include_str!("../../desktop/src/repository/product.rs"),
        include_str!("../src/repository/product.rs"),
    ),
    (
        "order",
        include_str!("../../core/src/order/model.rs"),
        include_str!("../../desktop/src/repository/order.rs"),
        include_str!("../src/repository/order.rs"),
    ),
    (
        "wallet",
        include_str!("../../core/src/wallet/model.rs"),
        include_str!("../../desktop/src/repository/wallet.rs"),
        include_str!("../src/repository/wallet.rs"),
    ),
    (
        "treasury",
        include_str!("../../core/src/treasury/model.rs"),
        include_str!("../../desktop/src/repository/treasury.rs"),
        include_str!("../src/repository/treasury.rs"),
    ),
    (
        "finance",
        include_str!("../../core/src/finance/model.rs"),
        include_str!("../../desktop/src/repository/finance.rs"),
        include_str!("../src/repository/finance.rs"),
    ),
    (
        "cash",
        include_str!("../../core/src/cash/model.rs"),
        concat!(
            include_str!("../../desktop/src/repository/cash_session.rs"),
            include_str!("../../desktop/src/repository/cash_movement.rs"),
        ),
        concat!(
            include_str!("../src/repository/cash_session.rs"),
            include_str!("../src/repository/cash_movement.rs"),
        ),
    ),
    (
        "coupon",
        include_str!("../../core/src/coupon/model.rs"),
        include_str!("../../desktop/src/repository/coupon.rs"),
        include_str!("../src/repository/coupon.rs"),
    ),
    (
        "subscription",
        include_str!("../../core/src/subscription/model.rs"),
        include_str!("../../desktop/src/repository/subscription.rs"),
        include_str!("../src/repository/subscription.rs"),
    ),
];

/// Campos dos structs PERSISTIDOS do modelo.
///
/// Persistido = tem `base: BaseFields` (§6: id, company_id, timestamps,
/// `deleted_at`, `synced`). Structs derivados/agregados do mesmo arquivo
/// (resumos de tela, DTOs de payload) ficam de fora — eles não têm tabela.
fn campos_do_modelo(src: &str) -> Vec<String> {
    let mut campos = Vec::new();
    for bloco in src.split("pub struct ").skip(1) {
        let Some(fim) = bloco.find("\n}") else { continue };
        let corpo = &bloco[..fim];
        // Persistido = tem `base: BaseFields` OU carrega os próprios
        // campos de identidade/tempo (caso de `Company`, que é o próprio
        // tenant e não usa `BaseFields`).
        let persistido = corpo.contains("base: BaseFields")
            || (corpo.contains("pub id: Uuid") && corpo.contains("pub created_at:"));
        if !persistido {
            continue;
        }
        for linha in corpo.lines().map(str::trim) {
            let Some(resto) = linha.strip_prefix("pub ") else { continue };
            let Some((nome, _)) = resto.split_once(':') else { continue };
            let nome = nome.trim();
            if !nome.is_empty() && nome.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                campos.push(nome.to_string());
            }
        }
    }
    campos
}

#[test]
fn todo_campo_do_modelo_viaja_no_sync() {
    let mut faltando: Vec<String> = Vec::new();
    let mut conferidos = 0usize;

    for (entidade, modelo, repo_sqlite, repo_pg) in ENTIDADES {
        let campos = campos_do_modelo(modelo);
        assert!(
            !campos.is_empty(),
            "nenhum campo persistido reconhecido em `{entidade}` — o parser \
             quebrou e o teste passaria vazio"
        );
        for campo in campos {
            conferidos += 1;
            let isento = FORA_DO_UPSERT
                .iter()
                .any(|(e, c, _)| (*e == *entidade || *e == "*") && *c == campo);
            if isento {
                continue;
            }
            for (banco, src) in [("SQLite", repo_sqlite), ("PostgreSQL", repo_pg)] {
                if !src.contains(&campo) {
                    faltando.push(format!("{entidade}.{campo} ausente no repositório {banco}"));
                }
            }
        }
    }

    // Guarda contra o teste passar por não ter conferido nada.
    assert!(
        conferidos >= 100,
        "só {conferidos} campos conferidos — esperado >= 100; o parser do \
         modelo provavelmente quebrou"
    );
    assert!(
        faltando.is_empty(),
        "campos do modelo que NÃO aparecem no repositório (nunca sincronizam):\n  {}",
        faltando.join("\n  ")
    );
}
