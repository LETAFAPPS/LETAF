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

/// Id do horário de funcionamento de um dia — `(company_id, day_of_week)`.
pub fn business_hours(company_id: Uuid, day_of_week: i32) -> Uuid {
    Uuid::new_v5(&NS_LETAF, format!("business_hours:{company_id}:{day_of_week}").as_bytes())
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
