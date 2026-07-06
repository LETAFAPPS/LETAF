//! Handlers da tela "Plano & cobrança" (Fase 13).
//!
//! Regras aplicadas (AI_RULES.md §1, §8, §11, §14):
//! - UI nunca contém lógica de negócio. Aqui apenas: coleta dados do
//!   SubscriptionService, formata para os structs Slint e roteia
//!   callbacks (escolher plano, trocar cartão, baixar PDF).
//! - O desktop é cliente burro: nenhuma chamada direta à Efi — tudo passa
//!   pelo server (mTLS + OAuth + credenciais).
//!
//! Dividido por meio de cobrança (AI_RULES.md §8, §9):
//! - `plans`: orquestrador (`setup_subscription`), refresh e seleção de plano
//! - `pix`: modal de PIX imediato (cobrança avulsa de fatura)
//! - `card`: cartão recorrente (+ helpers compartilhados `toast`/`refresh`)
//! - `pix_auto`: mandato de Pix Automático
//! - `payment_methods`: CRUD de formas de pagamento

mod card;
mod payment_methods;
mod pix;
mod pix_auto;
mod plans;

pub(crate) use plans::setup_subscription;
