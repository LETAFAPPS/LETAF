//! Domínio Assinatura — plano da empresa + histórico de faturas.
//!
//! Regras aplicadas (AI_RULES.md §1, §6, §9):
//! - model + repository (trait) + service.
//! - Catálogo de planos (Mensal/Semestral/Anual) fica como constante
//!   no service até o painel do super administrador existir. Quando
//!   migrar para tabela `plans`, a interface do service não muda —
//!   apenas a fonte dos dados.
//! - Cobrança real (gateway) será integrada em fase posterior; esta
//!   fase entrega apenas o acompanhamento + escolha do plano.

pub mod card_billing;
pub mod model;
pub mod pix_auto_billing;
pub mod repository;
pub mod service;
