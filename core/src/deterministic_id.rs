//! Ids DETERMINÍSTICOS para entidades de chave natural (§6, §7).
//!
//! Entidades singleton ou com chave natural única (a carteira de um
//! cliente, a tesouraria da empresa, o horário de um dia) eram criadas
//! com `Uuid::new_v4()` em cada terminal. Dois terminais offline geravam
//! ids DIFERENTES para a mesma coisa e, ao sincronizar:
//!
//! - o upsert por `id` batia no índice único natural → erro → o registro
//!   nunca subia e o pull da entidade congelava do outro lado;
//! - resolvendo por chave natural, cada lado ficava com o próprio id — e
//!   os movimentos filhos apontavam para um `account_id` que o servidor
//!   não conhecia, então o delta NUNCA era aplicado ao saldo (falha
//!   silenciosa, sem FK para acusar).
//!
//! Derivando o id da própria chave natural, os dois terminais chegam ao
//! MESMO id sem se falar, e a classe inteira de problema desaparece.
//!
//! Registros antigos (id aleatório) continuam válidos: o upsert por
//! chave natural segue casando com eles.

use uuid::Uuid;

/// Namespace do projeto para os ids derivados (UUIDv5, RFC 4122).
/// Constante arbitrária e ESTÁVEL — mudá-la reescreveria todos os ids.
const NS_LETAF: Uuid = Uuid::from_bytes([
    0x1e, 0x74, 0xaf, 0x00, 0x5c, 0x21, 0x4b, 0x9a, 0x8d, 0x33, 0x9f, 0x10, 0x62, 0x77, 0xc4, 0x51,
]);

/// Id da carteira de um cliente — chave natural `(company_id, customer_id)`.
pub fn wallet_account(company_id: Uuid, customer_id: Uuid) -> Uuid {
    Uuid::new_v5(&NS_LETAF, format!("wallet_account:{company_id}:{customer_id}").as_bytes())
}

/// Id da tesouraria da empresa — singleton por `company_id`.
pub fn treasury_account(company_id: Uuid) -> Uuid {
    Uuid::new_v5(&NS_LETAF, format!("treasury_account:{company_id}").as_bytes())
}

/// Id da conta a receber AUTOMÁTICA do fiado — `(company_id, customer_id)`.
///
/// A conta espelha o saldo negativo da carteira. Vale UMA ABERTA por cliente
/// de cada vez, mas as encerradas ficam como histórico financeiro (cada uma é
/// um fiado que foi efetivamente recebido) — por isso o id não pode ser fixo
/// por cliente, senão um novo ciclo de dívida sobrescreveria o recebimento
/// anterior.
///
/// O discriminador é quantas contas automáticas o cliente já acumulou. Dois
/// terminais que ainda não se enxergaram partem do mesmo histórico (dado já
/// sincronizado), derivam o MESMO id e o upsert dedupa — em vez de criarem
/// duas contas abertas para a mesma dívida, dobrando o "a receber" e deixando
/// a segunda aberta para sempre (a busca por chave natural só acha a
/// primeira). Se os históricos já divergirem, cai no comportamento antigo.
pub fn fiado_auto_entry(company_id: Uuid, customer_id: Uuid, sequencial: usize) -> Uuid {
    Uuid::new_v5(
        &NS_LETAF,
        format!("fiado_auto_entry:{company_id}:{customer_id}:{sequencial}").as_bytes(),
    )
}

/// Id de uma CATEGORIA FINANCEIRA SEMENTE — `(company_id, nome)`.
///
/// Vale só para as seeds fixas do sistema ("Aluguel", "Insumos", …), não para
/// categorias que o usuário cria: essas não têm chave natural (o usuário pode
/// apagar e recriar com o mesmo nome) e seguem com id aleatório.
///
/// Sem isto, o 2º terminal instalado semeava OUTRAS 12 categorias com ids
/// novos — `seed_defaults` só olha o SQLite local, que está vazio antes do
/// primeiro pull — e a empresa ficava permanentemente com 24 categorias, cada
/// nome duplicado, em todos os terminais. Não era corrida: acontecia em 100%
/// das instalações seguintes.
pub fn finance_category_seed(company_id: Uuid, nome: &str) -> Uuid {
    Uuid::new_v5(&NS_LETAF, format!("finance_category_seed:{company_id}:{nome}").as_bytes())
}

/// Id do horário de funcionamento de um dia — `(company_id, day_of_week)`.
pub fn business_hours(company_id: Uuid, day_of_week: i32) -> Uuid {
    Uuid::new_v5(&NS_LETAF, format!("business_hours:{company_id}:{day_of_week}").as_bytes())
}

/// Id do movimento de estoque de um evento ÚNICO do pedido.
///
/// Só para eventos que acontecem UMA vez por pedido/produto — hoje o
/// cancelamento. Dois terminais cancelando o mesmo pedido geram o MESMO
/// id, e o `ON CONFLICT DO NOTHING` do servidor aplica o delta uma vez
/// só; com id aleatório, o estoque era restituído EM DOBRO.
///
/// NÃO use para `edit`: uma edição legítima pode acontecer várias vezes
/// no mesmo pedido/produto, e um id fixo colapsaria os deltas.
pub fn stock_movement_once(order_id: Uuid, reason: &str, product_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &NS_LETAF,
        format!("stock_movement:{order_id}:{reason}:{product_id}").as_bytes(),
    )
}

/// Id do movimento de INSUMO de um evento ÚNICO do pedido (restituição da
/// receita ao cancelar). Discrimina por `product_id` E `insumo_id`: dois
/// produtos do mesmo pedido que usam o mesmo insumo geram movimentos
/// distintos (ambos restituídos), e o mesmo pedido cancelado em dois
/// terminais gera o MESMO id (o servidor aplica o delta uma vez só).
/// Não use para `sale`/`edit` (podem repetir no mesmo pedido).
pub fn insumo_movement_once(order_id: Uuid, reason: &str, product_id: Uuid, insumo_id: Uuid) -> Uuid {
    Uuid::new_v5(
        &NS_LETAF,
        format!("insumo_movement:{order_id}:{reason}:{product_id}:{insumo_id}").as_bytes(),
    )
}

/// Id do movimento de caixa de um evento ÚNICO do pedido (estorno de
/// venda cancelada). Mesma razão do `stock_movement_once`.
pub fn cash_movement_once(order_id: Uuid, kind: &str) -> Uuid {
    Uuid::new_v5(
        &NS_LETAF,
        format!("cash_movement:{order_id}:{kind}").as_bytes(),
    )
}

/// Id da fatura de um CICLO de assinatura.
///
/// A idempotência da cobrança era "existe fatura neste MÊS?", o que
/// quebrava em plano de 7 dias (o 2º ciclo do mês nunca era faturado) e,
/// pior, o retorno antecipado não avançava o vencimento — o tick horário
/// reemitia cobrança PIX para a MESMA fatura, dezenas de vezes por dia.
/// Derivando o id do ciclo, a fatura é única por ciclo por construção.
pub fn subscription_invoice(subscription_id: Uuid, ciclo: chrono::NaiveDate) -> Uuid {
    Uuid::new_v5(
        &NS_LETAF,
        format!("subscription_invoice:{subscription_id}:{ciclo}").as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dois_terminais_chegam_ao_mesmo_id() {
        let empresa = Uuid::new_v4();
        let cliente = Uuid::new_v4();
        // Terminais diferentes, sem se falar, derivam o mesmo id.
        assert_eq!(
            wallet_account(empresa, cliente),
            wallet_account(empresa, cliente)
        );
        assert_eq!(treasury_account(empresa), treasury_account(empresa));
        assert_eq!(business_hours(empresa, 3), business_hours(empresa, 3));
    }

    #[test]
    fn cancelamento_em_dois_terminais_gera_o_mesmo_movimento() {
        let pedido = Uuid::new_v4();
        let produto = Uuid::new_v4();
        assert_eq!(
            stock_movement_once(pedido, "cancel", produto),
            stock_movement_once(pedido, "cancel", produto)
        );
        assert_eq!(
            cash_movement_once(pedido, "sale_reversal"),
            cash_movement_once(pedido, "sale_reversal")
        );
        // Produtos e motivos diferentes não colidem.
        assert_ne!(
            stock_movement_once(pedido, "cancel", produto),
            stock_movement_once(pedido, "cancel", Uuid::new_v4())
        );
        assert_ne!(
            stock_movement_once(pedido, "cancel", produto),
            stock_movement_once(pedido, "edit", produto)
        );
    }

    #[test]
    fn chaves_naturais_distintas_dao_ids_distintos() {
        let empresa = Uuid::new_v4();
        let outra = Uuid::new_v4();
        let cliente = Uuid::new_v4();
        assert_ne!(wallet_account(empresa, cliente), wallet_account(outra, cliente));
        assert_ne!(treasury_account(empresa), treasury_account(outra));
        assert_ne!(business_hours(empresa, 1), business_hours(empresa, 2));
        // E não colidem entre TIPOS de entidade.
        assert_ne!(
            treasury_account(empresa).to_string(),
            business_hours(empresa, 0).to_string()
        );
    }
}
